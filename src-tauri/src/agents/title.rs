use live_agent::notes::{log_step, logging_enabled};
use offline_agent::title::generate_title;
use roofle_core::storage::{QuestionStore, SessionRepository};
use tauri::{AppHandle, Emitter, Manager};

use super::client::configured_client;
use crate::events;
use crate::state::{AppState, TitleUpdate};

/// Rename a finished session from its notes. Skips silently when AI is
/// unconfigured. Runs on the notes agent thread: the title must be derived
/// only after the end-of-call notes are saved, because short sessions have no
/// mid-call note to read from.
pub(crate) fn rename_from_notes(app: AppHandle, session_id: i64) {
    let log = logging_enabled();
    let tag = format!("title session={session_id}");
    let Ok(client) = configured_client(&app, session_id) else {
        return;
    };

    let storage = &app.state::<AppState>().storage;
    let notes = storage
        .latest_note(session_id)
        .ok()
        .flatten()
        .map(|n| n.text)
        .unwrap_or_default();
    if notes.trim().is_empty() {
        return log_step(log, &tag, "no notes; skipping");
    }

    match generate_title(&client, &notes) {
        Ok(title) if !title.trim().is_empty() => {
            let title = title.trim();
            log_step(log, &tag, &format!("renamed to: {title}"));
            let _ = storage.set_title(session_id, title);
            let _ = app.emit(events::TITLE, TitleUpdate { session_id, title: title.to_string() });
        }
        Ok(_) => log_step(log, &tag, "empty title; skipping"),
        Err(e) => log_step(log, &tag, &e),
    }
}
