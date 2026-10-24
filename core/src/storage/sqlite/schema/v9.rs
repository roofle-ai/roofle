//! V9 migration — `llm_usage`: one row per LLM call, for the Settings usage
//! report. Rows outlive their session (the tokens were still spent), so
//! deleting a session only detaches them.

pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS llm_usage (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER REFERENCES sessions(id) ON DELETE SET NULL,
    model      TEXT    NOT NULL,
    tokens_in  INTEGER NOT NULL,
    tokens_out INTEGER NOT NULL,
    created_at TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_llm_usage_created ON llm_usage(created_at);
"#;
