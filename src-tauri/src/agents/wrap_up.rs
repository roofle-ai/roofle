use offline_agent::{parse_action_items, sections::summary_line};
use roofle_core::storage::SessionRepository;
use tauri::{AppHandle, Emitter, Manager};

use crate::events;
use crate::notify;
use crate::state::{AppState, WrapUp};

/// The session's notes document (agent output plus user edits); empty if none.
pub(crate) fn session_notes(app: &AppHandle, session_id: i64) -> String {
    app.state::<AppState>()
        .storage
        .get_session(session_id)
        .ok()
        .flatten()
        .map(|d| d.notes)
        .unwrap_or_default()
}

/// Close out a call: tell the UI its recipes can run, and, when the user is
/// not looking at Roofle, post a native notification. `ready` is whether the
/// final notes pass succeeded.
pub(crate) fn finish(app: &AppHandle, session_id: i64, ready: bool) {
    let notes = if ready { session_notes(app, session_id) } else { String::new() };
    let ready = ready && !notes.trim().is_empty();
    let action_items = parse_action_items(&notes).len();

    let _ = app.emit(events::WRAP_UP, WrapUp { session_id, ready, action_items });
    if ready {
        notify::notify_call_ready(app, summary_line(&notes).unwrap_or("Your notes are ready."), action_items);
    }
}
