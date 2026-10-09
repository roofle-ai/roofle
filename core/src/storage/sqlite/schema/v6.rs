//! V6 migration — `app_config`, a small key/value table for app-wide settings
//! that must survive restarts (e.g. the LLM API key).

pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS app_config (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;
