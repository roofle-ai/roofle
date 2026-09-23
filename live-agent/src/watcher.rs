//! The paragraph-boundary detector: decides when the engine should finalize
//! an accumulating paragraph and fold it into the merged transcript log.
//!
//! Per the db-persist plan, a paragraph accumulates recognized segments until
//! a boundary is detected (pause, punctuation, turn change), at which point it
//! is finalized. This watcher is a lightweight proxy for that boundary: it
//! counts characters since the last finalized paragraph and fires once
//! `char_limit` accumulate. The engine owns the actual persistence — it folds
//! each segment into the open paragraph and, on a boundary, finalizes it and
//! appends it to `transcript_log`.

/// Default number of *new* characters that trigger a paragraph boundary.
pub const DEFAULT_CHAR_LIMIT: usize = 200;

/// A per-session, incremental paragraph-boundary detector.
///
/// `push` consumes one finalized utterance and returns true once the buffered
/// *new* characters reach `char_limit`. The caller finalizes the open
/// paragraph, then calls [`reset`](Self::reset) so the next paragraph counts
/// from zero. [`has_pending`](Self::has_pending) reports un-finalized content
/// so the engine can flush the final paragraph when transcription stops.
#[derive(Debug)]
pub struct ParagraphWatcher {
    char_limit: usize,
    /// Characters accumulated since the last finalized paragraph.
    pending_chars: usize,
}

impl ParagraphWatcher {
    /// Start watching a session. `char_limit` is the new-character threshold
    /// (see [`DEFAULT_CHAR_LIMIT`]).
    pub fn new(char_limit: usize) -> Self {
        Self {
            char_limit,
            pending_chars: 0,
        }
    }

    /// Consume one finalized utterance. Returns true when a paragraph boundary
    /// is due: the engine should finalize the open paragraph and reset.
    pub fn push(&mut self, text: &str) -> bool {
        let t = text.trim();
        if t.is_empty() {
            return false;
        }
        self.pending_chars += t.chars().count();
        self.pending_chars >= self.char_limit
    }

    /// Reset the pending counter after a paragraph was finalized.
    pub fn reset(&mut self) {
        self.pending_chars = 0;
    }

    /// Whether any un-finalized content remains (for the stop-time flush).
    pub fn has_pending(&self) -> bool {
        self.pending_chars > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(n: usize) -> String {
        "x".repeat(n)
    }

    #[test]
    fn no_boundary_below_limit() {
        let mut w = ParagraphWatcher::new(DEFAULT_CHAR_LIMIT);
        assert!(!w.push(&block(50)));
        assert!(w.has_pending());
    }

    #[test]
    fn boundary_fires_exactly_at_limit() {
        let mut w = ParagraphWatcher::new(DEFAULT_CHAR_LIMIT);
        assert!(w.push(&block(DEFAULT_CHAR_LIMIT)));
    }

    #[test]
    fn reset_clears_pending() {
        let mut w = ParagraphWatcher::new(DEFAULT_CHAR_LIMIT);
        assert!(w.push(&block(DEFAULT_CHAR_LIMIT)));
        w.reset();
        assert!(!w.has_pending());
        // A fresh paragraph starts counting from zero again.
        assert!(!w.push(&block(50)));
    }

    #[test]
    fn boundary_accumulates_across_pushes() {
        let mut w = ParagraphWatcher::new(100);
        assert!(!w.push(&block(60)));
        assert!(w.push(&block(40)));
    }

    #[test]
    fn empty_text_is_ignored() {
        let mut w = ParagraphWatcher::new(1);
        assert!(!w.push("   "));
        assert!(!w.has_pending());
    }
}
