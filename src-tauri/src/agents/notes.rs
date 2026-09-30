use live_agent::notes::{agent_notes, log_step, logging_enabled};
use roofle_core::storage::QuestionStore;
use tauri::{AppHandle, Emitter, Manager};

use super::client::configured_client;
use crate::events;
use crate::state::{AppState, NotesUpdate};

/// Merge one new paragraph into the session's live notes and publish them.
/// Runs on a detached thread so a slow provider never blocks the engine.
pub fn generate(app: AppHandle, session_id: i64, seq: i64, new_text: String) {
    std::thread::spawn(move || {
        let log = logging_enabled();
        let tag = format!("notes #{seq} session={session_id}");
        let new_chars = new_text.chars().count();

        let client = match configured_client(&app) {
            Ok(client) => client,
            Err(reason) => {
                log_step(log, &tag, &format!("{reason}; skipping (new={new_chars} chars)"));
                return;
            }
        };
        log_step(
            log,
            &tag,
            &format!("agent start (model={}, new={new_chars} chars)", client.model()),
        );

        let storage = &app.state::<AppState>().storage;
        let existing = storage
            .latest_note(session_id)
            .ok()
            .flatten()
            .map(|n| n.text)
            .unwrap_or_default();

        match agent_notes(&client, &new_text, &existing, log) {
            Ok(text) => {
                log_step(log, &tag, &format!("final notes = {} chars", text.chars().count()));
                let _ = storage.save_note(session_id, seq, &text);
                let _ = app.emit(events::NOTES, NotesUpdate { seq, text });
            }
            Err(e) => log_step(log, &tag, &e),
        }
    });
}
