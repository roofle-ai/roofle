//! V5 migration — question pipeline persistence.
//!
//! Questions gain the pipeline's own key (`Q7`), the call-time of the line that
//! raised them, their gap type and the user's feedback. `session_memory` holds
//! the pipeline's per-session memory as JSON so a reload resumes identically;
//! `pipeline_runs` logs every LLM stage for tracing a bad question to its inputs.

pub(super) const SCHEMA: &str = r#"
ALTER TABLE questions ADD COLUMN question_key TEXT;
ALTER TABLE questions ADD COLUMN source_ts_ms INTEGER;
ALTER TABLE questions ADD COLUMN gap_type     TEXT;
ALTER TABLE questions ADD COLUMN feedback     TEXT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_questions_key
    ON questions (session_id, question_key) WHERE question_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS session_memory (
    session_id     INTEGER PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    memory_json    TEXT    NOT NULL,
    schema_version INTEGER NOT NULL,
    updated_at     TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS pipeline_runs (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id     INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    chunk_id       INTEGER NOT NULL,
    stage          TEXT    NOT NULL,
    model          TEXT    NOT NULL,
    prompt_version TEXT    NOT NULL,
    input          TEXT    NOT NULL,
    output         TEXT,
    error          TEXT,
    created_at     TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_pipeline_runs_session
    ON pipeline_runs (session_id, chunk_id);
"#;
