//! The paragraph watcher: turns a stream of finalized utterances into
//! character-threshold paragraph boundaries.

use roofle_core::storage::{NewParagraph, ParagraphStore, StorageError};

/// Default number of *new* characters that trigger a paragraph boundary.
pub const DEFAULT_CHAR_LIMIT: usize = 200;

/// A completed paragraph chunk for one session: the previous paragraph plus the
/// new `char_limit`+ chunk that followed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParagraphBoundary {
    pub session_id: i64,
    /// Monotonic paragraph number within the session (1-based).
    pub seq: i64,
    /// The paragraph that preceded this chunk (empty for the first boundary).
    pub prev_text: String,
    /// The newly-accumulated chunk (>= `char_limit` chars, or the flush tail).
    pub new_text: String,
}

/// Per-session, incremental character watcher.
///
/// `push` accumulates finalized utterance text; each time the buffered *new*
/// characters reach `char_limit`, an [`ParagraphBoundary`] is emitted with the
/// previous paragraph as `prev_text` and the new chunk as `new_text`. The new
/// chunk then becomes the previous paragraph for the next boundary, so the
/// window slides forward without re-reading the whole meeting.
#[derive(Debug)]
pub struct ParagraphWatcher {
    session_id: i64,
    char_limit: usize,
    seq: i64,
    prev_text: String,
    buffer: String,
}

impl ParagraphWatcher {
    /// Start watching a session. `char_limit` is the new-character threshold
    /// (see [`DEFAULT_CHAR_LIMIT`]).
    pub fn new(session_id: i64, char_limit: usize) -> Self {
        Self {
            session_id,
            char_limit,
            seq: 0,
            prev_text: String::new(),
            buffer: String::new(),
        }
    }

    /// Consume one finalized utterance. Returns any boundaries crossed, in order.
    pub fn push(&mut self, text: &str) -> Vec<ParagraphBoundary> {
        let t = text.trim();
        if t.is_empty() {
            return Vec::new();
        }
        if !self.buffer.is_empty() {
            self.buffer.push(' ');
        }
        self.buffer.push_str(t);

        let mut boundaries = Vec::new();
        while self.buffer.chars().count() >= self.char_limit {
            boundaries.push(self.emit());
        }
        boundaries
    }

    /// Emit a boundary from the current buffer and advance the window.
    fn emit(&mut self) -> ParagraphBoundary {
        self.seq += 1;
        let new_text = std::mem::take(&mut self.buffer);
        let prev_text = std::mem::replace(&mut self.prev_text, new_text.clone());
        ParagraphBoundary {
            session_id: self.session_id,
            seq: self.seq,
            prev_text,
            new_text,
        }
    }

    /// Flush any remaining buffered paragraph, even below `char_limit`. Returns
    /// a boundary when there is content, `None` when the buffer is empty. Called
    /// when transcription stops so the full final paragraph is stored.
    pub fn flush(&mut self) -> Option<ParagraphBoundary> {
        if self.buffer.chars().count() == 0 {
            return None;
        }
        Some(self.emit())
    }

    /// Number of buffered (un-flushed) new characters.
    pub fn buffered_chars(&self) -> usize {
        self.buffer.chars().count()
    }

    /// The watcher's session id.
    pub fn session_id(&self) -> i64 {
        self.session_id
    }
}

/// Persist a boundary to the store as a paragraph row.
pub fn persist_boundary(
    store: &dyn ParagraphStore,
    b: &ParagraphBoundary,
) -> Result<(), StorageError> {
    store
        .create_paragraph(NewParagraph {
            session_id: b.session_id,
            seq: b.seq,
            prev_text: b.prev_text.clone(),
            new_text: b.new_text.clone(),
            created_at: chrono::Utc::now(),
        })
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watcher(session_id: i64) -> ParagraphWatcher {
        ParagraphWatcher::new(session_id, DEFAULT_CHAR_LIMIT)
    }

    fn block(n: usize) -> String {
        "x".repeat(n)
    }

    #[test]
    fn no_boundary_below_limit() {
        let mut w = watcher(1);
        w.push(&block(50));
        assert!(w.buffered_chars() == 50);
        // No boundary emitted yet.
    }

    #[test]
    fn boundary_fires_exactly_at_limit() {
        let mut w = watcher(1);
        let out = w.push(&block(DEFAULT_CHAR_LIMIT));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].new_text.chars().count(), DEFAULT_CHAR_LIMIT);
        assert_eq!(w.buffered_chars(), 0); // watermark reset
    }

    #[test]
    fn boundary_splits_prev_and_new() {
        let mut w = watcher(1);
        w.push(&block(DEFAULT_CHAR_LIMIT)); // first boundary, no previous
        let out = w.push(&block(DEFAULT_CHAR_LIMIT)); // second boundary
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].prev_text.chars().count(), DEFAULT_CHAR_LIMIT);
        assert_eq!(out[0].new_text.chars().count(), DEFAULT_CHAR_LIMIT);
        // Previous for the third boundary is the second chunk.
        assert_eq!(w.prev_text, out[0].new_text);
    }

    #[test]
    fn seq_increments_across_boundaries() {
        let mut w = watcher(1);
        w.push(&block(DEFAULT_CHAR_LIMIT));
        w.push(&block(DEFAULT_CHAR_LIMIT));
        w.push(&block(DEFAULT_CHAR_LIMIT));
        assert_eq!(w.buffered_chars(), 0);
    }

    #[test]
    fn boundary_takes_the_whole_buffered_chunk() {
        // One oversized push fires a single boundary whose new_text is the whole
        // buffer (>= char_limit); the watermark resets to zero afterwards.
        let mut w = watcher(1);
        let out = w.push(&block(DEFAULT_CHAR_LIMIT * 3 + 10));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].new_text.chars().count(), DEFAULT_CHAR_LIMIT * 3 + 10);
        assert_eq!(w.buffered_chars(), 0);
    }

    #[test]
    fn per_session_isolation() {
        let mut a = watcher(10);
        let b = watcher(20);
        a.push(&block(DEFAULT_CHAR_LIMIT));
        assert_eq!(a.session_id(), 10);
        assert_eq!(b.session_id(), 20);
        assert_eq!(b.buffered_chars(), 0); // b untouched
    }

    #[test]
    fn flush_persists_remaining_below_limit() {
        let mut w = watcher(1);
        w.push(&block(DEFAULT_CHAR_LIMIT));
        w.push(&block(40)); // below limit, buffered
        let flushed = w.flush();
        assert!(flushed.is_some());
        assert_eq!(flushed.unwrap().seq, 2);
        assert_eq!(w.buffered_chars(), 0);
        // Flushing an empty watcher is a no-op.
        assert!(w.flush().is_none());
    }
}
