//! Post-call recipe drafts (`session_recipes`), keyed by session and recipe.
//!
//! The recipe is an opaque key so `roofle-core` stays independent of the
//! `offline-agent` crate that owns the typed `Recipe`.

use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use crate::storage::StorageError;

use super::sql::{SELECT_SESSION_RECIPE, UPSERT_SESSION_RECIPE};
use super::{to_rfc3339, SqliteStorage};

/// A stored draft: the email subject (if any) and the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRecipe {
    pub subject: Option<String>,
    pub body: String,
}

impl SqliteStorage {
    /// Upsert the draft for `recipe` (one row per session and recipe).
    pub fn save_recipe(
        &self,
        session_id: i64,
        recipe: &str,
        subject: Option<&str>,
        body: &str,
    ) -> Result<(), StorageError> {
        let conn = self.conn.lock().unwrap();
        let now = to_rfc3339(Utc::now());
        conn.execute(UPSERT_SESSION_RECIPE, params![session_id, recipe, subject, body, now])?;
        Ok(())
    }

    /// The stored draft for `recipe`, if it was generated before.
    pub fn load_recipe(&self, session_id: i64, recipe: &str) -> Result<Option<StoredRecipe>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(SELECT_SESSION_RECIPE, params![session_id, recipe], |r| {
                Ok(StoredRecipe { subject: r.get(0)?, body: r.get(1)? })
            })
            .optional()?;
        Ok(row)
    }
}
