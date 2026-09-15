//! SQLite storage. Owns the connection and migrations only; each domain's row
//! mapping lives in a dedicated child module — [`sessions`] (sessions +
//! segments), [`paragraphs`] — and every SQL string lives in [`sql`]. This file
//! stays about "the connection", never about a table's queries.

mod paragraphs;
mod sessions;
mod sql;

use chrono::{DateTime, Utc};
use std::path::Path;
use std::sync::Mutex;

use crate::storage::StorageError;

use sql::{SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4};

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

    /// Apply pending migrations, tracked by `PRAGMA user_version`.
    fn migrate(&self) -> Result<(), StorageError> {
        let mut conn = self.conn.lock().unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;

        if version < 1 {
            apply(&mut conn, SCHEMA_V1, 1)?;
        }
        if version < 2 {
            apply(&mut conn, SCHEMA_V2, 2)?;
        }
        if version < 3 {
            apply(&mut conn, SCHEMA_V3, 3)?;
        }
        if version < 4 {
            apply(&mut conn, SCHEMA_V4, 4)?;
        }
        Ok(())
    }

    /// Current schema version (`PRAGMA user_version`); 0 means a fresh DB.
    pub fn schema_version(&self) -> Result<i64, StorageError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }
}

/// Run a migration batch and stamp `user_version`, atomically.
fn apply(
    conn: &mut rusqlite::Connection,
    batch: &str,
    version: i64,
) -> Result<(), StorageError> {
    let tx = conn.transaction()?;
    tx.execute_batch(batch)?;
    tx.pragma_update(None, "user_version", version)?;
    tx.commit()?;
    Ok(())
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
