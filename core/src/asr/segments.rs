//! Typed transcription segments produced by the ASR engine.

use std::fmt;

use crate::storage::AudioSource;

/// Whether a segment is a running hypothesis or a stable, finalized utterance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SegmentKind {
    Partial,
    Final,
}

/// A single piece of recognized text and its stability kind.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Segment {
    pub kind: SegmentKind,
    pub source: AudioSource,
    pub text: String,
}

impl Segment {
    pub fn partial(source: AudioSource, text: impl Into<String>) -> Self {
        Self {
            kind: SegmentKind::Partial,
            source,
            text: text.into(),
        }
    }

    pub fn final_(source: AudioSource, text: impl Into<String>) -> Self {
        Self {
            kind: SegmentKind::Final,
            source,
            text: text.into(),
        }
    }
}

impl fmt::Display for Segment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.text)
    }
}

/// A completed transcription: the joined text plus its segments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transcript {
    pub text: String,
    pub segments: Vec<Segment>,
}

impl Transcript {
    pub fn from_final(text: impl Into<String>) -> Self {
        let text = text.into();
        // No live capture source for whole-file transcription; default to mic.
        let segment = Segment::final_(AudioSource::Mic, text.clone());
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
