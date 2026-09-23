//! Rolling summary + follow-up question persistence (db-persist).
//!
//! [`RollingSummary`]: one row per session, overwritten per chunk.
//! [`Question`]: one row per follow-up, with an `open`→`answered`/`stale` cycle.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::storage::StorageError;

/// Lifecycle of a follow-up question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionStatus {
    Open,
    Answered,
    Stale,
}

/// Rolling summary state — one row per session, overwritten on each chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollingSummary {
    pub session_id: i64,
    pub summary_text: String,
    /// Last `transcript_log` sequence folded into the summary.
    pub last_sequence_no: i64,
    pub updated_at: DateTime<Utc>,
}

/// The session's live note — one row per session, overridden each generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveNote {
    pub session_id: i64,
    /// `transcript_log` sequence this note was folded from.
    pub sequence_no: i64,
    pub text: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A follow-up question with its resolution state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: i64,
    pub session_id: i64,
    pub text: String,
    /// Which framework lens produced it, if any.
    pub lens: Option<String>,
    pub status: QuestionStatus,
    pub resolution_reason: Option<String>,
    /// `transcript_log` sequence that generated this question.
    pub source_sequence_no: i64,
    /// `transcript_log` sequence that resolved it, if resolved.
    pub resolved_sequence_no: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Persistence for rolling summaries and follow-up questions.
pub trait QuestionStore {
    /// Load a session's rolling summary, if one has been produced.
    fn load_rolling_summary(&self, session_id: i64) -> Result<Option<RollingSummary>, StorageError>;

    /// Overwrite a session's rolling summary, recording the last sequence
    /// folded in.
    fn save_rolling_summary(
        &self,
        session_id: i64,
        summary_text: &str,
        last_sequence_no: i64,
    ) -> Result<(), StorageError>;

    /// A session's questions, newest first.
    fn list_questions(&self, session_id: i64) -> Result<Vec<Question>, StorageError>;

    /// Insert one open question for a session.
    fn create_question(
        &self,
        session_id: i64,
        text: &str,
        source_sequence_no: i64,
    ) -> Result<Question, StorageError>;

    /// Mark every currently-open question of a session stale. Used when the
    /// challenge agent emits a fresh list, so superseded questions close out.
    fn mark_open_questions_stale(&self, session_id: i64) -> Result<(), StorageError>;

    /// Resolve one question: set its status, reason, and the transcript
    /// sequence that resolved it (when answered).
    fn resolve_question(
        &self,
        id: i64,
        status: QuestionStatus,
        reason: &str,
        resolved_sequence_no: Option<i64>,
    ) -> Result<(), StorageError>;

    /// Append one live-note generation for a session.
    fn save_note(
        &self,
        session_id: i64,
        sequence_no: i64,
        text: &str,
    ) -> Result<LiveNote, StorageError>;

    /// The latest live note of a session (the running notes doc).
    fn latest_note(&self, session_id: i64) -> Result<Option<LiveNote>, StorageError>;
}
