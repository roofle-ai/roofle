//! Derives the essay/paragraph view from ordered, sentence-level audio segments.

use crate::storage::AudioSegment;

/// Render ordered audio segments as meeting-notes text.
///
/// Segments are VAD utterances (sentence-sized). Joining them with a single
/// space reads as flowing prose; a segment that ends with sentence punctuation
/// closes the current paragraph, so the transcript reads as one sentence per
/// paragraph.
pub fn render_paragraph(segments: &[AudioSegment]) -> String {
    let mut out = String::new();

    for seg in segments {
        let text = seg.text.trim();
        if text.is_empty() {
            continue;
        }

        match out.chars().last() {
            // Previous segment ended a sentence: start a new paragraph.
            Some(c) if matches!(c, '.' | '!' | '?') => out.push_str("\n\n"),
            Some(_) => out.push(' '),
            None => {}
        }
        out.push_str(text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn seg(text: &str) -> AudioSegment {
        AudioSegment {
            id: 0,
            session_id: 1,
            source: crate::storage::AudioSource::Mic,
            text: text.to_string(),
            start_ms: 0,
            end_ms: 0,
            confidence: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn joins_sentence_segments_with_spaces() {
        let out = render_paragraph(&[seg("hello"), seg("world")]);
        assert_eq!(out, "hello world");
    }

    #[test]
    fn breaks_paragraph_after_sentence_punctuation() {
        let out = render_paragraph(&[seg("First."), seg("Second!"), seg("third")]);
        assert_eq!(out, "First.\n\nSecond!\n\nthird");
    }

    #[test]
    fn skips_empty_segments() {
        let out = render_paragraph(&[seg(""), seg("kept"), seg("   ")]);
        assert_eq!(out, "kept");
    }

    #[test]
    fn empty_input_is_empty_output() {
        assert_eq!(render_paragraph(&[]), "");
    }
}
