//! Every SQL string in the store, in one place. Schema batches are applied in
//! order by `SqliteStorage::migrate`; query constants are named by table + verb.

// --- Schema (applied in version order) ------------------------------------

/// v1: sessions + segments; timestamps are RFC 3339 UTC text so the file stays
/// human-readable; segments cascade-delete with their session.
pub(super) const SCHEMA_V1: &str = r#"
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
pub(super) const SCHEMA_V2: &str = r#"
ALTER TABLE sessions ADD COLUMN updated_at TEXT;
UPDATE sessions SET updated_at = started_at WHERE updated_at IS NULL;
"#;

/// v3: derived paragraph chunks written by live-agent's paragraph watcher.
/// One row per character-threshold boundary; keyed per session so a session's
/// paragraphs can be replayed in order and cascade-delete with their session.
pub(super) const SCHEMA_V3: &str = r#"
CREATE TABLE IF NOT EXISTS paragraphs (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    prev_text  TEXT    NOT NULL,
    new_text   TEXT    NOT NULL,
    created_at TEXT    NOT NULL,
    UNIQUE (session_id, seq)
);

CREATE INDEX IF NOT EXISTS idx_paragraphs_session ON paragraphs (session_id, seq);
"#;

/// v4: the editable notes document per session (seed/transcript + user edits).
/// Defaults to empty; transcription itself remains in `segments` (unchanged).
pub(super) const SCHEMA_V4: &str = r#"
ALTER TABLE sessions ADD COLUMN notes TEXT NOT NULL DEFAULT '';
"#;

// --- sessions -------------------------------------------------------------

/// Insert a session row; the id comes back via `last_insert_rowid`.
pub(super) const INSERT_SESSION: &str = r#"
INSERT INTO sessions (title, source, model_id, started_at, status, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6)
"#;

/// Close a session: set `ended_at`, accumulate duration, mark completed.
pub(super) const UPDATE_SESSION_FINALIZE: &str = r#"
UPDATE sessions
SET ended_at = ?1, duration_secs = duration_secs + ?2,
    status = 'completed', updated_at = ?1
WHERE id = ?3
"#;

/// Read a session's accumulated duration (for the continue offset).
pub(super) const SELECT_SESSION_DURATION: &str =
    "SELECT duration_secs FROM sessions WHERE id = ?1";

/// Reopen a stored session for continued recording.
pub(super) const UPDATE_SESSION_RESUME: &str =
    "UPDATE sessions SET status = 'recording', updated_at = ?1 WHERE id = ?2";

/// Sessions by most recent activity, with a first-words preview.
pub(super) const SELECT_SESSIONS: &str = r#"
SELECT s.id, s.title, s.source, s.model_id, s.started_at, s.ended_at,
       s.duration_secs, s.status, s.updated_at,
       (SELECT text FROM segments WHERE session_id = s.id ORDER BY seq LIMIT 1)
FROM sessions s
ORDER BY s.updated_at DESC
LIMIT ?1
"#;

/// One session row by id (includes the editable notes document).
pub(super) const SELECT_SESSION: &str = r#"
SELECT id, title, source, model_id, started_at, ended_at, duration_secs, status, updated_at, notes
FROM sessions
WHERE id = ?1
"#;

/// Persist the editable notes document for a session.
pub(super) const UPDATE_SESSION_NOTES: &str =
    "UPDATE sessions SET notes = ?1 WHERE id = ?2";

/// Delete a session; children cascade.
pub(super) const DELETE_SESSION: &str = "DELETE FROM sessions WHERE id = ?1";

// --- segments -------------------------------------------------------------

/// Append one finalized utterance.
pub(super) const INSERT_SEGMENT: &str = r#"
INSERT INTO segments (session_id, seq, start_secs, text, created_at)
VALUES (?1, ?2, ?3, ?4, ?5)
"#;

/// Highest segment sequence of a session (0 when it has none).
pub(super) const SELECT_LAST_SEG_SEQ: &str =
    "SELECT COALESCE(MAX(seq), 0) FROM segments WHERE session_id = ?1";

/// A session's segments in recognition order.
pub(super) const SELECT_SEGMENTS: &str = r#"
SELECT id, session_id, seq, start_secs, text, created_at
FROM segments
WHERE session_id = ?1
ORDER BY seq
"#;

// --- paragraphs -----------------------------------------------------------

/// Insert one paragraph chunk.
pub(super) const INSERT_PARAGRAPH: &str = r#"
INSERT INTO paragraphs (session_id, seq, prev_text, new_text, created_at)
VALUES (?1, ?2, ?3, ?4, ?5)
"#;

/// A session's paragraphs in sequence order.
pub(super) const SELECT_PARAGRAPHS: &str = r#"
SELECT id, session_id, seq, prev_text, new_text, created_at
FROM paragraphs
WHERE session_id = ?1
ORDER BY seq
"#;
