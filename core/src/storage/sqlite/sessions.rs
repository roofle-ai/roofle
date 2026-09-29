//! Session + segment persistence ([`SessionRepository`]). Lives in a child
//! module of `sqlite` so it can reach the private connection handle while the
//! connection file stays small.

use chrono::{DateTime, Utc};
use rusqlite::params;

use crate::storage::{
    render_paragraph, NewSegment, NewSession, Session, SessionDetail, SessionRepository,
    SessionStatus, SessionSummary, StorageError, TranscriptSegment,
};

use super::sql::{
    DELETE_SESSION, INSERT_SEGMENT, INSERT_SESSION, SELECT_LAST_SEG_SEQ, SELECT_SEGMENTS,
    SELECT_SESSION, SELECT_SESSIONS, SELECT_SESSION_DURATION, UPDATE_SESSION_FINALIZE,
    UPDATE_SESSION_NOTES, UPDATE_SESSION_RESUME, UPDATE_SESSION_TITLE,
};
use super::{parse_utc, to_rfc3339, SqliteStorage};

/// Preview length: first `PREVIEW_WORDS` words of a transcript.
const PREVIEW_WORDS: usize = 10;

impl SessionRepository for SqliteStorage {
    fn create_session(&self, input: NewSession) -> Result<Session, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            INSERT_SESSION,
            params![
                "",
                input.source,
                input.model_id,
                to_rfc3339(input.started_at),
                status_str(SessionStatus::Recording),
                to_rfc3339(input.started_at),
            ],
        )?;

        Ok(Session {
            id: conn.last_insert_rowid(),
            title: String::new(),
            source: input.source,
            model_id: input.model_id,
            started_at: input.started_at,
            ended_at: None,
            duration_secs: 0.0,
            status: SessionStatus::Recording,
            updated_at: input.started_at,
        })
    }

    fn append_segments(&self, segments: &[NewSegment]) -> Result<(), StorageError> {
        if segments.is_empty() {
            return Ok(());
        }

        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(INSERT_SEGMENT)?;
            for seg in segments {
                stmt.execute(params![
                    seg.session_id,
                    seg.seq,
                    seg.start_secs,
                    seg.text,
                    to_rfc3339(seg.created_at),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn finalize_session(
        &self,
        id: i64,
        ended_at: DateTime<Utc>,
        duration_secs: f64,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        // Duration accumulates so a continued session keeps its total; the
        // finalize time is also the session's `updated_at`.
        let n = conn.execute(
            UPDATE_SESSION_FINALIZE,
            params![to_rfc3339(ended_at), duration_secs, id],
        )?;
        if n == 0 {
            return Err(StorageError::NotFound(id));
        }
        Ok(())
    }

    fn continue_session(&self, id: i64) -> Result<f64, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(SELECT_SESSION_DURATION)?;
        let duration = match stmt.query_row(params![id], |r| r.get::<_, f64>(0)) {
            Ok(d) => d,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Err(StorageError::NotFound(id)),
            Err(e) => return Err(StorageError::Sqlite(e)),
        };
        conn.execute(
            UPDATE_SESSION_RESUME,
            params![to_rfc3339(Utc::now()), id],
        )?;
        Ok(duration)
    }

    fn last_seq(&self, session_id: i64) -> Result<i64, StorageError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            SELECT_LAST_SEG_SEQ,
            params![session_id],
            |r| r.get(0),
        )?)
    }

    fn list_sessions(&self, limit: Option<i64>) -> Result<Vec<SessionSummary>, StorageError> {
        let conn = self.conn.lock().unwrap();
        // Preview = first segment's text, so the list shows the opening words
        // without fetching the whole transcript.
        let mut stmt = conn.prepare(SELECT_SESSIONS)?;

        // `LIMIT -1` means unlimited in SQLite.
        let rows = stmt.query_map(params![limit.unwrap_or(-1)], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, f64>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, Option<String>>(9)?,
            ))
        })?;
        rows.map(|r| {
            let (
                id,
                title,
                source,
                model_id,
                started,
                ended,
                duration_secs,
                status,
                updated,
                preview,
            ) = r?;
            Ok(SessionSummary {
                id,
                title,
                source,
                model_id,
                started_at: parse_utc(&started)?,
                ended_at: ended.as_deref().map(parse_utc).transpose()?,
                duration_secs,
                status: parse_status(&status),
                updated_at: parse_utc(&updated)?,
                preview: preview_words(preview.as_deref().unwrap_or(""), PREVIEW_WORDS),
            })
        })
        .collect()
    }

    fn save_notes(&self, id: i64, notes: &str) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(UPDATE_SESSION_NOTES, params![notes, id])?;
        if n == 0 {
            return Err(StorageError::NotFound(id));
        }
        Ok(())
    }

    fn set_title(&self, id: i64, title: &str) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(UPDATE_SESSION_TITLE, params![title, id])?;
        if n == 0 {
            return Err(StorageError::NotFound(id));
        }
        Ok(())
    }

    fn get_session(&self, id: i64) -> Result<Option<SessionDetail>, StorageError> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(SELECT_SESSION)?;
        let mut rows = stmt.query_map(params![id], read_session)?;
        let Some(raw) = rows.next() else {
            return Ok(None);
        };
        let (session, notes) = build_session(raw?)?;

        let mut stmt = conn.prepare(SELECT_SEGMENTS)?;
        let segments = stmt
            .query_map(params![id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, f64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })?
            .map(|r| {
                let (id, session_id, seq, start_secs, text, created_at) = r?;
                Ok(TranscriptSegment {
                    id,
                    session_id,
                    seq,
                    start_secs,
                    text,
                    created_at: parse_utc(&created_at)?,
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()?;

        Ok(Some(SessionDetail {
            transcript: render_paragraph(&segments),
            session,
            segments,
            notes,
        }))
    }

    fn delete_session(&self, id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(DELETE_SESSION, params![id])?;
        if n == 0 {
            return Err(StorageError::NotFound(id));
        }
        Ok(())
    }
}

/// Read a session's raw columns, deferring timestamp/status parsing to callers
/// (query_map closures can only return `rusqlite::Result`).
fn read_session(
    r: &rusqlite::Row,
) -> rusqlite::Result<(
    i64,
    String,
    String,
    String,
    String,
    Option<String>,
    f64,
    String,
    String,
    String,
)> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
    ))
}

/// Build the session plus its editable notes document from a raw row.
fn build_session(
    (id, title, source, model_id, started, ended, duration_secs, status, updated, notes): (
        i64,
        String,
        String,
        String,
        String,
        Option<String>,
        f64,
        String,
        String,
        String,
    ),
) -> Result<(Session, String), StorageError> {
    Ok((
        Session {
            id,
            title,
            source,
            model_id,
            started_at: parse_utc(&started)?,
            ended_at: ended.as_deref().map(parse_utc).transpose()?,
            duration_secs,
            status: parse_status(&status),
            updated_at: parse_utc(&updated)?,
        },
        notes,
    ))
}

/// First `max` whitespace-separated words of `text`.
fn preview_words(text: &str, max: usize) -> String {
    text.split_whitespace().take(max).collect::<Vec<_>>().join(" ")
}

fn status_str(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Recording => "recording",
        SessionStatus::Completed => "completed",
    }
}

fn parse_status(s: &str) -> SessionStatus {
    match s {
        "recording" => SessionStatus::Recording,
        _ => SessionStatus::Completed,
    }
}
