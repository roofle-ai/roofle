// roofle-app — offline title agent.
//
// from its meeting notes. Reads the session's running notes, asks the LLM for
// a short title (via offline-agent), and persists it on the session row.

use live_agent::notes::{log_step, logging_enabled};
use offline_agent::title::generate_title;
use roofle_core::storage::{QuestionStore, SessionRepository};
use tauri::{AppHandle, Manager};

use crate::ai::load_config;
use crate::ai_client::{ai_data_dir, NotesClient};
use crate::state::AppState;

/// Rename a finished session from its meeting notes. Runs once on stop, on a
/// detached thread so a slow provider never blocks the engine loop. Skips when
/// AI is unconfigured or the session has no notes yet.
pub fn rename_session(app: AppHandle, session_id: i64) {
    std::thread::spawn(move || {
        let log = logging_enabled();
        // Same shared API key captured at recording start.
        let api_key = app.state::<AppState>().api_key.lock().unwrap().clone();
        let Some(client) = (match ai_data_dir(&app) {
            Ok(dir) => {
                let config = load_config(&dir);
                if config.model.trim().is_empty() || api_key.trim().is_empty() {
                    None
                } else {
                    Some(NotesClient::new(config, api_key))
                }
            }
            Err(_) => None,
        }) else {
            return;
        };

        // The running meeting-notes doc produced by the live notes agent.
        let storage = &app.state::<AppState>().storage;
        let notes = storage
            .latest_note(session_id)
            .ok()
            .flatten()
            .map(|n| n.text)
            .unwrap_or_default();
        if notes.trim().is_empty() {
            log_step(log, &format!("title session={session_id}"), "no notes; skipping");
            return;
        }

        match generate_title(&client, &notes) {
            Ok(title) if !title.trim().is_empty() => {
                log_step(
                    log,
                    &format!("title session={session_id}"),
                    &format!("renamed to: {title}"),
                );
                let _ = storage.set_title(session_id, title.trim());
            }
            Ok(_) => log_step(log, &format!("title session={session_id}"), "empty title; skipping"),
            Err(e) => log_step(log, &format!("title session={session_id}"), &e),
        }
    });
}
