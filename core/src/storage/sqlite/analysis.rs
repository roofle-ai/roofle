//! Offline self-assessment persistence (`session_analysis`).
//!
//! The result is stored as raw JSON text so `roofle-core` stays independent of
//! the `offline-agent` crate that owns the typed `AnalysisResult`.

use chrono::Utc;
use rusqlite::params;

use crate::storage::{StorageError, StoredAnalysis};

use super::sql::{SELECT_SESSION_ANALYSIS, UPSERT_SESSION_ANALYSIS};
use super::{to_rfc3339, SqliteStorage};

impl SqliteStorage {
    /// Upsert a session's offline analysis (one row per session).
    pub fn save_analysis(
        &self,
        session_id: i64,
        context: &str,
        result_json: &str,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let now = to_rfc3339(Utc::now());
        conn.execute(
            UPSERT_SESSION_ANALYSIS,
            params![session_id, context, result_json, now],
        )?;
        Ok(())
    }

    /// Load a session's persisted offline analysis, if any.
    pub fn load_analysis(&self, session_id: i64) -> Result<Option<StoredAnalysis>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(SELECT_SESSION_ANALYSIS)?;
        let mut rows = stmt.query(params![session_id])?;
        match rows.next()? {
            Some(row) => Ok(Some(StoredAnalysis {
                context: row.get(0)?,
                result_json: row.get(1)?,
            })),
            None => Ok(None),
        }
    }
}
