//! V8 migration — `topics`: user-created labels a session can be filed under
//! (at most one per session). Deleting a topic leaves its sessions untagged.

pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS topics (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    created_at TEXT NOT NULL
);

ALTER TABLE sessions ADD COLUMN topic_id INTEGER REFERENCES topics(id) ON DELETE SET NULL;
"#;
