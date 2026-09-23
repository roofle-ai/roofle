//! V2 migration — persisted offline self-assessments.
//!
//! One row per session (overwritten on re-analysis). The result is stored as
//! raw JSON text so `roofle-core` stays independent of the `offline-agent`
//! crate that defines the typed result.

/// The V2 delta: a table holding a session's latest offline analysis.
pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS session_analysis (
    session_id  INTEGER PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    context     TEXT    NOT NULL,
    result_json TEXT    NOT NULL,
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL
);
"#;
