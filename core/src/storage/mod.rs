
#[cfg(test)]
mod tests;

mod paragraph;
mod paragraph_store;
mod questions;
mod repository;
mod sqlite;
mod transcript;

pub use paragraph::render_paragraph;
pub use paragraph_store::{AudioSegment, NewAudioSegment, Paragraph, ParagraphStore};
pub use questions::{
    LiveNote, NewPipelineQuestion, PipelineRun, Question, QuestionStatus, QuestionStore,
};
pub use repository::{SessionRepository, StorageError};
pub use sqlite::{SessionUsage, StoredAnalysis, StoredRecipe, Topic};
pub use sqlite::SqliteStorage;
pub use transcript::{TranscriptEntry, TranscriptStore};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Which capture source produced a transcript unit. Per the db-persist plan a
/// segment/paragraph belongs to exactly one source and never mixes mic+system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioSource {
    /// The user's own microphone.
    Mic,
    /// System/output audio — the other call participant(s).
    System,
}

/// Lifecycle of a recording session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    /// The session row is open; recording is live.
    Recording,
    /// Recording stopped cleanly; `ended_at` and `duration_secs` are set.
    Completed,
}

/// One recorded meeting, opened on Start, closed on Stop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: i64,
    /// Optional human label; empty until a future feature sets it.
    pub title: String,
    /// Capture source: `mic`, `system` or `both`.
    pub source: String,
    /// ASR model id used for this session (from the model catalog).
    pub model_id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_secs: f64,
    pub status: SessionStatus,
    /// Last time the session was written to; advances when a stored session is
    /// continued with a new recording.
    pub updated_at: DateTime<Utc>,
}

/// A session with its full ordered, source-tagged audio-segment list and the
/// essay view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    pub session: Session,
    pub segments: Vec<AudioSegment>,
    /// [`render_paragraph`] over `segments`.
    pub transcript: String,
    /// The editable notes document (seed/transcript + user edits).
    pub notes: String,
}

/// A session as shown in the history list: metadata plus a first-words preview.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: i64,
    pub title: String,
    /// Capture source: `mic`, `system` or `both`.
    pub source: String,
    /// ASR model id used for this session.
    pub model_id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_secs: f64,
    pub status: SessionStatus,
    /// Last activity time; the history list shows this so a continued session's
    /// date/time updates.
    pub updated_at: DateTime<Utc>,
    /// First words of the transcript, for the history list preview.
    pub preview: String,
    /// The user-assigned topic, if any.
    pub topic_id: Option<i64>,
}

/// Input for creating a session row.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub source: String,
    pub model_id: String,
    pub started_at: DateTime<Utc>,
}
