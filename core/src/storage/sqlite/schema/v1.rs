//! V1 baseline schema — the current SQL, consolidated. Immutable: new changes
//! arrive as migrations (see `mod.rs`).

/// The complete V1 schema: every table in its final shape, created idempotently.
pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    title         TEXT    NOT NULL DEFAULT '',
    source        TEXT    NOT NULL,
    model_id      TEXT    NOT NULL,
    started_at    TEXT    NOT NULL,
    ended_at      TEXT,
    duration_secs REAL    NOT NULL DEFAULT 0,
    status        TEXT    NOT NULL DEFAULT 'recording',
    updated_at    TEXT    NOT NULL DEFAULT '',
    notes         TEXT    NOT NULL DEFAULT ''
);

-- Single per-session paragraph, overridden as it accumulates (db-persist).
-- On finalize its text moves to transcript_log and the row resets.
CREATE TABLE IF NOT EXISTS paragraphs (
    session_id  INTEGER PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    source      TEXT    NOT NULL CHECK (source IN ('mic', 'system')),
    text        TEXT    NOT NULL,
    start_ms    INTEGER NOT NULL,
    end_ms      INTEGER NOT NULL,
    sequence_no INTEGER NOT NULL DEFAULT 0,  -- last transcript_log seq folded
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL,
    CHECK (end_ms >= start_ms)
);

-- Raw ASR output units, source-tagged, folded into the session's paragraph.
CREATE TABLE IF NOT EXISTS audio_segments (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id    INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    source        TEXT    NOT NULL CHECK (source IN ('mic', 'system')),
    text          TEXT    NOT NULL,
    start_ms      INTEGER NOT NULL,
    end_ms        INTEGER NOT NULL,
    confidence    REAL,
    created_at    TEXT    NOT NULL,
    CHECK (end_ms >= start_ms)
);

CREATE INDEX IF NOT EXISTS idx_segments_session_source
    ON audio_segments (session_id, source, start_ms);

-- Merged, chronologically-ordered transcript log feeding the LLM agents.
CREATE TABLE IF NOT EXISTS transcript_log (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id    INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    source        TEXT    NOT NULL CHECK (source IN ('mic', 'system')),
    text          TEXT    NOT NULL,
    start_ms      INTEGER NOT NULL,
    sequence_no   INTEGER NOT NULL,
    created_at    TEXT    NOT NULL,
    UNIQUE (session_id, sequence_no)
);

CREATE INDEX IF NOT EXISTS idx_transcript_log_session_seq
    ON transcript_log (session_id, sequence_no);

-- Live notes, one row per session, overridden per note generation.
CREATE TABLE IF NOT EXISTS live_notes (
    session_id  INTEGER PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    sequence_no INTEGER NOT NULL,   -- transcript_log seq this note folded from
    text        TEXT    NOT NULL,
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL
);

-- Follow-up questions with a resolution lifecycle.
CREATE TABLE IF NOT EXISTS questions (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id           INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    text                 TEXT    NOT NULL,
    lens                 TEXT,
    status               TEXT    NOT NULL DEFAULT 'open'
                             CHECK (status IN ('open', 'answered', 'stale')),
    resolution_reason    TEXT,
    source_sequence_no   INTEGER NOT NULL,
    resolved_sequence_no INTEGER,
    created_at           TEXT    NOT NULL,
    updated_at           TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_questions_session_status
    ON questions (session_id, status);
"#;
