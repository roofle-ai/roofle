//! Repository abstraction over session storage.

use chrono::DateTime;
use chrono::Utc;
use std::fmt;

use crate::storage::{NewSession, Session, SessionDetail, SessionSummary};

/// Persistence error. Backend-facing; callers map it to a user-facing string.
#[derive(Debug)]
pub enum StorageError {
    /// DB file or directory access failed.
    Io(std::io::Error),
    /// SQLite returned an error.
    Sqlite(rusqlite::Error),
    /// No row with the given id.
    NotFound(i64),
    /// A timestamp column held unparseable text.
    BadTimestamp(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Io(e) => write!(f, "io: {e}"),
            StorageError::Sqlite(e) => write!(f, "sqlite: {e}"),
            StorageError::NotFound(id) => write!(f, "no session with id {id}"),
            StorageError::BadTimestamp(s) => write!(f, "bad timestamp in db: {s}"),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StorageError::Io(e) => Some(e),
            StorageError::Sqlite(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        StorageError::Io(e)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(e: rusqlite::Error) -> Self {
        StorageError::Sqlite(e)
    }
}

/// Session persistence, implemented by [`SqliteStorage`](crate::storage::SqliteStorage).
pub trait SessionRepository {
    /// Open a session row; returns it with its generated id.
    fn create_session(&self, input: NewSession) -> Result<Session, StorageError>;

    /// Close a session: set `ended_at`, duration and status.
    fn finalize_session(
        &self,
        id: i64,
        ended_at: DateTime<Utc>,
        duration_secs: f64,
    ) -> Result<(), StorageError>;

    /// Reopen a stored session for continued recording: mark it recording,
    /// bump `updated_at`, and return its accumulated duration (used as the
    /// timestamp offset for the new block).
    fn continue_session(&self, id: i64) -> Result<f64, StorageError>;

    /// Sessions by most recent activity, with a first-words preview; `limit`
    /// caps the row count when set.
    fn list_sessions(&self, limit: Option<i64>) -> Result<Vec<SessionSummary>, StorageError>;

    /// One session with its ordered segments and rendered paragraph text.
    fn get_session(&self, id: i64) -> Result<Option<SessionDetail>, StorageError>;

    /// Persist the editable notes document for a session.
    fn save_notes(&self, id: i64, notes: &str) -> Result<(), StorageError>;

    /// Rename a session's title (set by the offline title-generator agent).
    fn set_title(&self, id: i64, title: &str) -> Result<(), StorageError>;

    /// Delete a session; its segments cascade.
    fn delete_session(&self, id: i64) -> Result<(), StorageError>;
}
