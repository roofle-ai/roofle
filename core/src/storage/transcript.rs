//! Merged, chronologically-ordered transcript log (db-persist).
//!
//! The LLM pipeline reads one chronological view, not the per-source tables.
//! Finalized paragraphs interleave here by time, ordered by `sequence_no`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::storage::{AudioSource, StorageError};

/// One finalized paragraph folded into the merged transcript log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub id: i64,
    pub session_id: i64,
    pub source: AudioSource,
    pub text: String,
    pub start_ms: i64,
    /// Monotonic order within the session; resolves same-timestamp ties.
    pub sequence_no: i64,
    pub created_at: DateTime<Utc>,
}

/// Persistence for the merged transcript log.
pub trait TranscriptStore {
    /// Highest transcript sequence of a session (0 when it has none).
    fn last_transcript_sequence(&self, session_id: i64) -> Result<i64, StorageError>;

    /// Append a finalized paragraph to the merged log, assigning the next
    /// sequence number. Returns the sequence number assigned.
    fn append_transcript(
        &self,
        session_id: i64,
        source: AudioSource,
        text: &str,
        start_ms: i64,
    ) -> Result<i64, StorageError>;

    /// The newest (highest-sequence) transcript entry of a session.
    fn newest_transcript(&self, session_id: i64) -> Result<Option<TranscriptEntry>, StorageError>;

    /// Transcript entries after (not including) `after_sequence`, in order.
    fn transcript_after(
        &self,
        session_id: i64,
        after_sequence: i64,
    ) -> Result<Vec<TranscriptEntry>, StorageError>;
}
