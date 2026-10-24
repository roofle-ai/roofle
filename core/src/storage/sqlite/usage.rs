//! LLM token usage (`llm_usage`): recorded per call, reported per session.

use chrono::Utc;
use rusqlite::params;
use serde::Serialize;

use crate::storage::StorageError;

use super::{to_rfc3339, SqliteStorage};

/// One session's totals for a month. `session_id` is `None` for calls made
/// outside a session, and for sessions deleted since.
#[derive(Debug, Clone, Serialize)]
pub struct SessionUsage {
    pub session_id: Option<i64>,
    pub title: String,
    pub started_at: Option<String>,
    pub calls: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

impl SqliteStorage {
    pub fn record_usage(
        &self,
        session_id: Option<i64>,
        model: &str,
        tokens_in: u64,
        tokens_out: u64,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO llm_usage (session_id, model, tokens_in, tokens_out, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, model, tokens_in as i64, tokens_out as i64, to_rfc3339(Utc::now())],
        )?;
        Ok(())
    }

    /// Earliest month (`YYYY-MM`, UTC) with any usage, if there is any.
    pub fn usage_first_month(&self) -> Result<Option<String>, StorageError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("SELECT MIN(substr(created_at, 1, 7)) FROM llm_usage", [], |r| r.get(0))?)
    }

    /// All-time `(tokens_in, tokens_out)` for one session.
    pub fn usage_for_session(&self, session_id: i64) -> Result<(i64, i64), StorageError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(tokens_in), 0), COALESCE(SUM(tokens_out), 0)
             FROM llm_usage WHERE session_id = ?1",
            params![session_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    /// Totals per session for `month` (`YYYY-MM`, UTC), newest session first.
    pub fn usage_by_session(&self, month: &str) -> Result<Vec<SessionUsage>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT u.session_id, COALESCE(s.title, ''), s.started_at,
                    COUNT(*), SUM(u.tokens_in), SUM(u.tokens_out)
             FROM llm_usage u LEFT JOIN sessions s ON s.id = u.session_id
             WHERE substr(u.created_at, 1, 7) = ?1
             GROUP BY u.session_id
             ORDER BY MAX(u.created_at) DESC",
        )?;
        let rows = stmt.query_map(params![month], |r| {
            Ok(SessionUsage {
                session_id: r.get(0)?,
                title: r.get(1)?,
                started_at: r.get(2)?,
                calls: r.get(3)?,
                tokens_in: r.get(4)?,
                tokens_out: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}
