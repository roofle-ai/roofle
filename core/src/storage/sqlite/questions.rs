//! Question + live-note persistence ([`QuestionStore`]).
//! Child module of `sqlite` so it shares the private connection handle.

use rusqlite::params;

use crate::storage::{LiveNote, Question, QuestionStatus, QuestionStore, StorageError};

use super::sql::{
    INSERT_QUESTION, SELECT_LATEST_LIVE_NOTE, SELECT_QUESTIONS, UPDATE_QUESTION_RESOLVE,
    UPDATE_QUESTIONS_STALE, UPSERT_LIVE_NOTE,
};
use super::{parse_utc, to_rfc3339, SqliteStorage};

impl QuestionStore for SqliteStorage {
    fn list_questions(&self, session_id: i64) -> Result<Vec<Question>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(SELECT_QUESTIONS)?;
        let rows = stmt.query_map(params![session_id], read_question)?;
        rows.map(|r| r.map_err(StorageError::from)).collect()
    }

    fn create_question(
        &self,
        session_id: i64,
        text: &str,
        source_sequence_no: i64,
    ) -> Result<Question, StorageError> {
        let conn = self.conn.lock().unwrap();
        let now = to_rfc3339(chrono::Utc::now());
        conn.execute(
            INSERT_QUESTION,
            params![session_id, text, None::<String>, source_sequence_no, now],
        )?;
        Ok(Question {
            id: conn.last_insert_rowid(),
            session_id,
            text: text.to_string(),
            lens: None,
            status: QuestionStatus::Open,
            resolution_reason: None,
            source_sequence_no,
            resolved_sequence_no: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
    }

    fn mark_open_questions_stale(&self, session_id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            UPDATE_QUESTIONS_STALE,
            params![to_rfc3339(chrono::Utc::now()), session_id],
        )?;
        Ok(())
    }

    fn resolve_question(
        &self,
        id: i64,
        status: QuestionStatus,
        reason: &str,
        resolved_sequence_no: Option<i64>,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            UPDATE_QUESTION_RESOLVE,
            params![
                status_str(status),
                reason,
                resolved_sequence_no,
                to_rfc3339(chrono::Utc::now()),
                id,
            ],
        )?;
        Ok(())
    }

    fn save_note(
        &self,
        session_id: i64,
        sequence_no: i64,
        text: &str,
    ) -> Result<LiveNote, StorageError> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now();
        conn.execute(
            UPSERT_LIVE_NOTE,
            params![session_id, sequence_no, text, to_rfc3339(now)],
        )?;
        Ok(LiveNote {
            session_id,
            sequence_no,
            text: text.to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    fn latest_note(&self, session_id: i64) -> Result<Option<LiveNote>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(SELECT_LATEST_LIVE_NOTE)?;
        let mut rows = stmt.query_map(params![session_id], read_live_note)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }
}

fn read_question(row: &rusqlite::Row) -> rusqlite::Result<Question> {
    let status: String = row.get(4)?;
    Ok(Question {
        id: row.get(0)?,
        session_id: row.get(1)?,
        text: row.get(2)?,
        lens: row.get(3)?,
        status: parse_status(&status),
        resolution_reason: row.get(5)?,
        source_sequence_no: row.get(6)?,
        resolved_sequence_no: row.get(7)?,
        created_at: parse_utc(&row.get::<_, String>(8)?)
            .map_err(ts_err)?,
        updated_at: parse_utc(&row.get::<_, String>(9)?)
            .map_err(ts_err)?,
    })
}

fn parse_status(s: &str) -> QuestionStatus {
    match s {
        "answered" => QuestionStatus::Answered,
        "stale" => QuestionStatus::Stale,
        _ => QuestionStatus::Open,
    }
}

fn status_str(status: QuestionStatus) -> &'static str {
    match status {
        QuestionStatus::Open => "open",
        QuestionStatus::Answered => "answered",
        QuestionStatus::Stale => "stale",
    }
}

fn read_live_note(row: &rusqlite::Row) -> rusqlite::Result<LiveNote> {
    Ok(LiveNote {
        session_id: row.get(0)?,
        sequence_no: row.get(1)?,
        text: row.get(2)?,
        created_at: parse_utc(&row.get::<_, String>(3)?).map_err(ts_err)?,
        updated_at: parse_utc(&row.get::<_, String>(4)?).map_err(ts_err)?,
    })
}

fn ts_err(e: StorageError) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string())))
}
