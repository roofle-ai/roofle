//! Source-tagged ASR units and the session's single paragraph (db-persist).
//!
//! Each [`AudioSegment`] belongs to one source and folds into the session's
//! one [`Paragraph`] row, overridden as it accumulates. On a boundary the
//! engine finalizes it: the text moves to `transcript_log` and the row resets
//! for the next paragraph (see [`TranscriptStore`]).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::storage::{AudioSource, StorageError};

/// A raw ASR output unit — the smallest transcript granularity, source-tagged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSegment {
    pub id: i64,
    pub session_id: i64,
    pub source: AudioSource,
    pub text: String,
    /// Offset from session start, milliseconds.
    pub start_ms: i64,
    /// Offset from session start, milliseconds.
    pub end_ms: i64,
    /// ASR confidence score, if available.
    pub confidence: Option<f64>,
    pub created_at: DateTime<Utc>,
}

/// Input for inserting one recognized audio segment.
#[derive(Debug, Clone)]
pub struct NewAudioSegment {
    pub session_id: i64,
    pub source: AudioSource,
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub confidence: Option<f64>,
    pub created_at: DateTime<Utc>,
}

/// The session's single, accumulating paragraph. One row per session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paragraph {
    pub session_id: i64,
    pub source: AudioSource,
    /// Concatenation of its segments so far.
    pub text: String,
    pub start_ms: i64,
    pub end_ms: i64,
    /// Last `transcript_log` sequence folded into the paragraph.
    pub sequence_no: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Persistence for audio segments and the session's single paragraph.
pub trait ParagraphStore {
    /// Insert a source-tagged audio segment and fold its text into the
    /// session's paragraph (creating the row on first use).
    fn add_segment(&self, seg: &NewAudioSegment) -> Result<(), StorageError>;

    /// Finalize the session's paragraph: append its text to the merged
    /// transcript log (new sequence), reset the paragraph row, and return
    /// `(sequence_no, text)` — or `None` when the paragraph is empty.
    fn finalize_paragraph(&self, session_id: i64) -> Result<Option<(i64, String)>, StorageError>;
}
