//! Question + live-note persistence ([`QuestionStore`]).
//! Child module of `sqlite` so it shares the private connection handle.

use rusqlite::params;

use crate::storage::{
    LiveNote, NewPipelineQuestion, PipelineRun, Question, QuestionStatus, QuestionStore,
    StorageError,
};

use super::sql::{
    INSERT_PIPELINE_QUESTION, INSERT_PIPELINE_RUN, SELECT_LATEST_LIVE_NOTE,
    SELECT_MEMORY, SELECT_QUESTIONS, UPDATE_QUESTION_FEEDBACK, UPDATE_QUESTION_RESOLVE_BY_KEY,
    UPSERT_LIVE_NOTE, UPSERT_MEMORY,
};
use super::{parse_utc, to_rfc3339, SqliteStorage};

impl QuestionStore for SqliteStorage {
    fn list_questions(&self, session_id: i64) -> Result<Vec<Question>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(SELECT_QUESTIONS)?;
        let rows = stmt.query_map(params![session_id], read_question)?;
        rows.map(|r| r.map_err(StorageError::from)).collect()
    }

    fn create_pipeline_question(&self, q: &NewPipelineQuestion) -> Result<i64, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            INSERT_PIPELINE_QUESTION,
            params![
                q.session_id,
                q.text,
                q.source_sequence_no,
                to_rfc3339(chrono::Utc::now()),
                q.key,
                q.source_ts_ms,
                q.gap_type,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    fn resolve_question_by_key(
        &self,
        session_id: i64,
        key: &str,
        status: QuestionStatus,
        reason: &str,
        resolved_sequence_no: Option<i64>,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            UPDATE_QUESTION_RESOLVE_BY_KEY,
            params![
                status_str(status),
                reason,
                resolved_sequence_no,
                to_rfc3339(chrono::Utc::now()),
                session_id,
                key,
            ],
        )?;
        Ok(())
    }

    fn set_question_feedback(
        &self,
        session_id: i64,
        key: &str,
        feedback: &str,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            UPDATE_QUESTION_FEEDBACK,
            params![feedback, to_rfc3339(chrono::Utc::now()), session_id, key],
        )?;
        Ok(())
    }

    fn save_memory(&self, session_id: i64, json: &str, schema_version: u32) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            UPSERT_MEMORY,
            params![session_id, json, schema_version, to_rfc3339(chrono::Utc::now())],
        )?;
        Ok(())
    }

    fn load_memory(&self, session_id: i64) -> Result<Option<String>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(SELECT_MEMORY)?;
        let mut rows = stmt.query_map(params![session_id], |r| r.get::<_, String>(0))?;
        rows.next().transpose().map_err(StorageError::from)
    }

    fn log_pipeline_run(&self, run: &PipelineRun) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            INSERT_PIPELINE_RUN,
            params![
                run.session_id,
                run.chunk_id,
                run.stage,
                run.model,
                run.prompt_version,
                run.input,
                run.output,
                run.error,
                to_rfc3339(chrono::Utc::now()),
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
        question_key: row.get(10)?,
        source_ts_ms: row.get(11)?,
        feedback: row.get(12)?,
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
