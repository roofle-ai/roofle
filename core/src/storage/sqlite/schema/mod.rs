//! Database initialization: the V1 baseline schema and future migrations.
//!
//! [`initialize`] applies migrations in order, tracking progress with
//! `PRAGMA user_version`. Fresh DBs run every step; existing DBs run only the
//! steps they are missing, so they upgrade instead of being rebuilt.

mod v1;
mod v2;
mod v3;
mod v4;

/// Apply all pending migrations and stamp `user_version` accordingly.
pub(super) fn initialize(
    conn: &mut rusqlite::Connection,
) -> Result<(), crate::storage::StorageError> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;

    let tx = conn.transaction()?;
    if version < 1 {
        tx.execute_batch(v1::SCHEMA)?;
        tx.pragma_update(None, "user_version", 1)?;
    }
    if version < 2 {
        tx.execute_batch(v2::SCHEMA)?;
        tx.pragma_update(None, "user_version", 2)?;
    }
    if version < 3 {
        tx.execute_batch(v3::SCHEMA)?;
        tx.pragma_update(None, "user_version", 3)?;
    }
    if version < 4 {
        tx.execute_batch(v4::SCHEMA)?;
        tx.pragma_update(None, "user_version", 4)?;
    }
    tx.commit()?;
    Ok(())
}
