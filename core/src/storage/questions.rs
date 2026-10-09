//! Follow-up question + live-note persistence (db-persist).
//!
//! [`Question`]: one row per follow-up, with an `open`→`answered`/`stale` cycle.
//! [`LiveNote`]: the session's running notes document, overridden per chunk.

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
    /// The question pipeline's own id (`Q7`), when it came from there.
    pub question_key: Option<String>,
    /// Call time (ms) of the line that raised it; finer than the paragraph start.
    pub source_ts_ms: Option<i64>,
    /// `asked` or `dismissed`.
    pub feedback: Option<String>,
}

/// A question surfaced by the pipeline.
#[derive(Debug, Clone)]
pub struct NewPipelineQuestion<'a> {
    pub session_id: i64,
    pub key: &'a str,
    pub text: &'a str,
    pub gap_type: &'a str,
    pub source_sequence_no: i64,
    pub source_ts_ms: i64,
}

/// One logged LLM stage call.
#[derive(Debug, Clone)]
pub struct PipelineRun<'a> {
    pub session_id: i64,
    pub chunk_id: i64,
    pub stage: &'a str,
    pub model: &'a str,
    pub prompt_version: &'a str,
    pub input: &'a str,
    pub output: Option<&'a str>,
    pub error: Option<&'a str>,
}

/// Persistence for rolling summaries and follow-up questions.
pub trait QuestionStore {
    /// A session's questions, newest first.
    fn list_questions(&self, session_id: i64) -> Result<Vec<Question>, StorageError>;

    /// Insert an open question raised by the pipeline; returns its row id.
    fn create_pipeline_question(&self, q: &NewPipelineQuestion) -> Result<i64, StorageError>;

    /// Resolve the pipeline question with this key (`Q7`) in a session.
    fn resolve_question_by_key(
        &self,
        session_id: i64,
        key: &str,
        status: QuestionStatus,
        reason: &str,
        resolved_sequence_no: Option<i64>,
    ) -> Result<(), StorageError>;

    /// Record user feedback on a pipeline question.
    fn set_question_feedback(
        &self,
        session_id: i64,
        key: &str,
        feedback: &str,
    ) -> Result<(), StorageError>;

    /// Overwrite the session's pipeline memory JSON.
    fn save_memory(&self, session_id: i64, json: &str, schema_version: u32) -> Result<(), StorageError>;

    /// The session's saved pipeline memory JSON, if any.
    fn load_memory(&self, session_id: i64) -> Result<Option<String>, StorageError>;

    /// Append one pipeline stage run to the trace log.
    fn log_pipeline_run(&self, run: &PipelineRun) -> Result<(), StorageError>;

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
