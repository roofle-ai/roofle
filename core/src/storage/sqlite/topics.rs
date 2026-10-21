//! User-defined topics (`topics`) and the session → topic tag.

use chrono::Utc;
use rusqlite::params;
use serde::Serialize;

use crate::storage::StorageError;

use super::{to_rfc3339, SqliteStorage};

#[derive(Debug, Clone, Serialize)]
pub struct Topic {
    pub id: i64,
    pub name: String,
}

impl SqliteStorage {
    /// All topics, alphabetical.
    pub fn list_topics(&self) -> Result<Vec<Topic>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name FROM topics ORDER BY name")?;
        let rows = stmt.query_map([], |r| Ok(Topic { id: r.get(0)?, name: r.get(1)? }))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Create a topic, or return the existing one with the same name (case-insensitive).
    pub fn create_topic(&self, name: &str) -> Result<Topic, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO topics (name, created_at) VALUES (?1, ?2)",
            params![name, to_rfc3339(Utc::now())],
        )?;
        Ok(conn.query_row("SELECT id, name FROM topics WHERE name = ?1", params![name], |r| {
            Ok(Topic { id: r.get(0)?, name: r.get(1)? })
        })?)
    }

    /// File a session under `topic_id`, or clear its topic with `None`.
    pub fn set_session_topic(&self, session_id: i64, topic_id: Option<i64>) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE sessions SET topic_id = ?1 WHERE id = ?2",
            params![topic_id, session_id],
        )?;
        if n == 0 {
            return Err(StorageError::NotFound(session_id));
        }
        Ok(())
    }

    /// Delete a topic; its sessions stay, untagged (`ON DELETE SET NULL`).
    pub fn delete_topic(&self, id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM topics WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(StorageError::NotFound(id));
        }
        Ok(())
    }
}
