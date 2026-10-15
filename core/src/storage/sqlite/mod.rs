//! SQLite storage. Owns the connection and schema initialization; each domain's
//! row mapping lives in a dedicated child module — [`sessions`] (sessions +
//! segments), [`paragraphs`], [`transcript`], [`questions`] — and every query
//! string lives in [`sql`]. This file stays about "the connection"; the schema
//! itself lives in the [`schema`] directory (DB initialization / migrations).

mod analysis;
mod app_config;
mod paragraphs;
mod questions;
mod recipes;
mod schema;
mod sessions;
mod sql;
mod transcript;

use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Mutex;

use crate::storage::StorageError;

pub use recipes::StoredRecipe;

/// A session's persisted offline self-assessment: the analysis context that
/// produced it plus the raw `AnalysisResult` JSON.
#[derive(Debug, Clone)]
pub struct StoredAnalysis {
    pub context: String,
    pub result_json: String,
}

/// Thread-safe SQLite store. Wrap in `app.manage` once, share everywhere.
pub struct SqliteStorage {
    conn: Mutex<rusqlite::Connection>,
}

impl SqliteStorage {
    /// Open (or create) the DB at `path`, applying any pending migrations.
    /// WAL journaling lets the engine writer run alongside UI readers.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }

        let conn = rusqlite::Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.migrate()?;
        Ok(storage)
    }

    /// In-memory store for tests (no WAL, which is a file-mode pragma).
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = rusqlite::Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.migrate()?;
        Ok(storage)
    }

    /// Initialize the schema on a fresh database. Future schema changes are
    /// applied here as incremental migrations (see the `schema` directory),
    /// tracked by `PRAGMA user_version`.
    fn migrate(&self) -> Result<(), StorageError> {
        let mut conn = self.conn.lock().unwrap();
        schema::initialize(&mut conn)
    }

    /// Current schema version (`PRAGMA user_version`); 0 means a fresh DB.
    pub fn schema_version(&self) -> Result<i64, StorageError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }
}

/// RFC 3339 UTC text, matching what `chrono`'s serde emits over IPC.
fn to_rfc3339(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

/// Parse RFC 3339 UTC text back into a `DateTime<Utc>`.
fn parse_utc(s: &str) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| StorageError::BadTimestamp(s.to_string()))
}
