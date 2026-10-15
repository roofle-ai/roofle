//! V7 migration — `session_recipes`: generated post-call drafts (recap email,
//! Slack post), one row per session and recipe, so each is generated once.

pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS session_recipes (
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    recipe     TEXT    NOT NULL,
    subject    TEXT,
    body       TEXT    NOT NULL,
    created_at TEXT    NOT NULL,
    PRIMARY KEY (session_id, recipe)
);
"#;
