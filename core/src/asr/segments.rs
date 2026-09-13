//! Typed transcription segments produced by the ASR engine.
//!
//! `design.md` pipeline tail:
//!
//! ```text
//! S[streaming ASR] -> E[Partial and final segments] -> F[Frontend transcript panel]
//! ```
//!
//! S5 introduces the types; S6 feeds live partial results from the streaming
//! engine; S9 emits them to the frontend as typed events.

use std::fmt;

/// Whether a segment is a running hypothesis or a stable, finalized utterance.
///
/// `Serialize` (S9) so the Tauri shell can emit segments to the frontend as
/// typed IPC events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum SegmentKind {
    /// Text that is still being recognized (the hypothesis may still change).
    Partial,
    /// A completed utterance (endpoint reached, or input finished).
    Final,
}

/// A single piece of recognized text and its stability kind.
///
/// `Serialize` (S9) so a segment crosses the Tauri IPC boundary unchanged.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Segment {
    pub kind: SegmentKind,
    pub text: String,
}

impl Segment {
    /// Build a partial (still-changing) segment.
    pub fn partial(text: impl Into<String>) -> Self {
        Self {
            kind: SegmentKind::Partial,
            text: text.into(),
        }
    }

    /// Build a final (stable) segment.
    pub fn final_(text: impl Into<String>) -> Self {
        Self {
            kind: SegmentKind::Final,
            text: text.into(),
        }
    }
}

impl fmt::Display for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.text)
    }
}

/// A completed transcription: the joined text plus the segments it was built
/// from. Used as the return value of offline (`transcribe_wav`) transcription.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transcript {
    /// Full transcript text (final segments joined with a space).
    pub text: String,
    /// The segments that make up this transcript.
    pub segments: Vec<Segment>,
}

impl Transcript {
    /// Build a transcript from a single final segment's text.
    pub fn from_final(text: impl Into<String>) -> Self {
        let text = text.into();
        let segment = Segment::final_(text.clone());
        Self {
            text,
            segments: vec![segment],
        }
    }
}

impl fmt::Display for Transcript {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.text)
    }
}
