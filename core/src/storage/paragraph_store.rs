//! Persistence for derived paragraph chunks (from `live-agent`'s paragraph
//! watcher). One row per boundary: the previous paragraph plus the new chunk,
//! keyed by session + sequence. Implemented by the single-writer `SqliteStorage`
//! so paragraphs, segments and (future) questions share one DB connection.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::storage::StorageError;

/// Input for storing one paragraph chunk for a session.
#[derive(Debug, Clone)]
pub struct NewParagraph {
    pub session_id: i64,
    /// Monotonic paragraph number within the session (1-based).
    pub seq: i64,
    /// The paragraph that preceded this chunk.
    pub prev_text: String,
    /// The newly-accumulated chunk.
    pub new_text: String,
    pub created_at: DateTime<Utc>,
}

/// A stored paragraph chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paragraph {
    pub id: i64,
    pub session_id: i64,
    pub seq: i64,
    pub prev_text: String,
    pub new_text: String,
    pub created_at: DateTime<Utc>,
}

/// Persistence for derived paragraph chunks.
pub trait ParagraphStore {
    /// Insert one paragraph chunk.
    fn create_paragraph(&self, input: NewParagraph) -> Result<Paragraph, StorageError>;

    /// All paragraphs of a session, in sequence order.
    fn list_paragraphs(&self, session_id: i64) -> Result<Vec<Paragraph>, StorageError>;
}
