//! App-wide key/value settings (`app_config`).

use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use crate::storage::StorageError;

use super::{to_rfc3339, SqliteStorage};

impl SqliteStorage {
    pub fn get_config(&self, key: &str) -> Result<Option<String>, StorageError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row("SELECT value FROM app_config WHERE key = ?1", params![key], |r| r.get(0))
            .optional()?)
    }

    pub fn set_config(&self, key: &str, value: &str) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO app_config (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, to_rfc3339(Utc::now())],
        )?;
        Ok(())
    }
}
