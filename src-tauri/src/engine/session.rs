use chrono::Utc;
use roofle_core::storage::{NewSession, SessionRepository};
use tauri::{AppHandle, Manager};

use super::control::SourceKind;
use crate::state::AppState;

pub struct SessionStart {
    pub id: i64,
    /// Accumulated duration of a continued session (0 for a fresh one).
    pub timestamp_offset: f64,
    pub last_seq: i64,
}

/// Open a fresh session row, or reopen a stored one for continuation.
pub fn prepare(
    app: &AppHandle,
    source: SourceKind,
    continue_id: Option<i64>,
) -> Result<SessionStart, String> {
    match continue_id {
        Some(id) => continue_existing(app, id),
        None => open_new(app, source).map(|id| SessionStart {
            id,
            timestamp_offset: 0.0,
            last_seq: 0,
        }),
    }
}

fn continue_existing(app: &AppHandle, id: i64) -> Result<SessionStart, String> {
    let storage = &app.state::<AppState>().storage;
    let context = |e| format!("continue session: {e}");
    Ok(SessionStart {
        id,
        timestamp_offset: storage.continue_session(id).map_err(context)?,
        last_seq: storage.last_seq(id).map_err(context)?,
    })
}

fn open_new(app: &AppHandle, source: SourceKind) -> Result<i64, String> {
    let state = app.state::<AppState>();
    let model_id = state.selected_model.lock().unwrap().clone();
    state
        .storage
        .create_session(NewSession {
            source: source.as_db_str().to_string(),
            model_id,
            started_at: Utc::now(),
        })
        .map(|s| s.id)
        .map_err(|e| format!("start session: {e}"))
}

pub fn close(app: &AppHandle, session_id: i64, duration_secs: f64) {
    let result = app
        .state::<AppState>()
        .storage
        .finalize_session(session_id, Utc::now(), duration_secs);
    if let Err(e) = result {
        eprintln!("[roofle] finalize session {session_id}: {e}");
    }
}
