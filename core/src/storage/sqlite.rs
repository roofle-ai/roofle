//! SQLite-backed [`SessionRepository`]. Owns all SQL; callers see domain types
//! only. A `Mutex<Connection>` makes the store shareable across the engine
//! thread (writer) and Tauri command threads (readers).

use chrono::{DateTime, Utc};
use rusqlite::params;
use std::path::Path;
use std::sync::Mutex;

use crate::storage::{
    render_paragraph, NewSegment, NewSession, Session, SessionDetail, SessionRepository,
    SessionStatus, SessionSummary, StorageError, TranscriptSegment,
};

/// First schema version. Timestamps are RFC 3339 UTC text so the file stays
/// human-readable; segments cascade-delete with their session.
const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    title         TEXT    NOT NULL DEFAULT '',
    source        TEXT    NOT NULL,
    model_id      TEXT    NOT NULL,
    started_at    TEXT    NOT NULL,
    ended_at      TEXT,
    duration_secs REAL    NOT NULL DEFAULT 0,
    status        TEXT    NOT NULL DEFAULT 'recording'
);

CREATE TABLE IF NOT EXISTS segments (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    start_secs REAL    NOT NULL,
    text       TEXT    NOT NULL,
    created_at TEXT    NOT NULL,
    UNIQUE (session_id, seq)
);

CREATE INDEX IF NOT EXISTS idx_segments_session ON segments (session_id, seq);
"#;

/// v2: `updated_at` lets a continued session's date/time refresh in the list.
const SCHEMA_V2: &str = r#"
ALTER TABLE sessions ADD COLUMN updated_at TEXT;
UPDATE sessions SET updated_at = started_at WHERE updated_at IS NULL;
"#;

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
            let tx = conn.transaction()?;
            tx.execute_batch(SCHEMA_V1)?;
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        }
        if version < 2 {
            let tx = conn.transaction()?;
            tx.execute_batch(SCHEMA_V2)?;
            tx.pragma_update(None, "user_version", 2)?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Current schema version (`PRAGMA user_version`); 0 means a fresh DB.
    pub fn schema_version(&self) -> Result<i64, StorageError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
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
    ))
}

fn build_session(
    (id, title, source, model_id, started, ended, duration_secs, status, updated): (
        i64,
        String,
        String,
        String,
        String,
        Option<String>,
        f64,
        String,
        String,
    ),
) -> Result<Session, StorageError> {
    Ok(Session {
        id,
        title,
        source,
        model_id,
        started_at: parse_utc(&started)?,
        ended_at: ended.as_deref().map(parse_utc).transpose()?,
        duration_secs,
        status: parse_status(&status),
        updated_at: parse_utc(&updated)?,
    })
}

impl SessionRepository for SqliteStorage {
    fn create_session(&self, input: NewSession) -> Result<Session, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (title, source, model_id, started_at, status, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
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
            let mut stmt = tx.prepare(
                "INSERT INTO segments (session_id, seq, start_secs, text, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
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
            "UPDATE sessions
             SET ended_at = ?1, duration_secs = duration_secs + ?2,
                 status = 'completed', updated_at = ?1
             WHERE id = ?3",
            params![to_rfc3339(ended_at), duration_secs, id],
        )?;
        if n == 0 {
            return Err(StorageError::NotFound(id));
        }
        Ok(())
    }

    fn continue_session(&self, id: i64) -> Result<f64, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT duration_secs FROM sessions WHERE id = ?1")?;
        let duration = match stmt.query_row(params![id], |r| r.get::<_, f64>(0)) {
            Ok(d) => d,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Err(StorageError::NotFound(id)),
            Err(e) => return Err(StorageError::Sqlite(e)),
        };
        conn.execute(
            "UPDATE sessions SET status = 'recording', updated_at = ?1 WHERE id = ?2",
            params![to_rfc3339(Utc::now()), id],
        )?;
        Ok(duration)
    }

    fn last_seq(&self, session_id: i64) -> Result<i64, StorageError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM segments WHERE session_id = ?1",
            params![session_id],
            |r| r.get(0),
        )?)
    }

    fn list_sessions(&self, limit: Option<i64>) -> Result<Vec<SessionSummary>, StorageError> {
        let conn = self.conn.lock().unwrap();
        // Preview = first segment's text, so the list shows the opening words
        // without fetching the whole transcript.
        let mut stmt = conn.prepare(
            "SELECT s.id, s.title, s.source, s.model_id, s.started_at, s.ended_at,
                    s.duration_secs, s.status, s.updated_at,
                    (SELECT text FROM segments WHERE session_id = s.id ORDER BY seq LIMIT 1)
             FROM sessions s
             ORDER BY s.updated_at DESC
             LIMIT ?1",
        )?;

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

    fn get_session(&self, id: i64) -> Result<Option<SessionDetail>, StorageError> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, title, source, model_id, started_at, ended_at, duration_secs, status,
                    updated_at
             FROM sessions
             WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], read_session)?;
        let Some(raw) = rows.next() else {
            return Ok(None);
        };
        let session = build_session(raw?)?;

        let mut stmt = conn.prepare(
            "SELECT id, session_id, seq, start_secs, text, created_at
             FROM segments
             WHERE session_id = ?1
             ORDER BY seq",
        )?;
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
        }))
    }

    fn delete_session(&self, id: i64) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(StorageError::NotFound(id));
        }
        Ok(())
    }
}

/// RFC 3339 UTC text, matching what `chrono`'s serde emits over IPC.
fn to_rfc3339(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

fn parse_utc(s: &str) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| StorageError::BadTimestamp(s.to_string()))
}

/// Preview length: first `PREVIEW_WORDS` words of a transcript.
const PREVIEW_WORDS: usize = 10;

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
