use std::sync::Mutex;

use live_agent::notes::{agent_notes, log_step, logging_enabled};
use roofle_core::storage::{AudioSource, QuestionStore, SessionRepository};
use tauri::{AppHandle, Emitter, Manager};

use super::client::configured_client;
use crate::events;
use crate::state::{AppState, NotesUpdate};

/// Serialises runs so a window-due call and the end-of-call call never merge
/// notes from the same base at once.
static RUN: Mutex<()> = Mutex::new(());

/// Update the session's live notes from the transcript since the last update.
/// Runs only once `notes_window_mins` of new transcript has built up, or when
/// `ended` (call over) and anything is pending. Detached so a slow provider
/// never blocks the engine.
pub fn generate(app: AppHandle, session_id: i64, ended: bool) {
    std::thread::spawn(move || {
        let _run = RUN.lock().unwrap_or_else(|e| e.into_inner());
        let log = logging_enabled();
        let state = app.state::<AppState>();
        let storage = &state.storage;

        let last = storage.latest_note(session_id).ok().flatten();
        let covered = last.as_ref().map_or(0, |n| n.sequence_no);
        let mut pending = match storage.get_session(session_id) {
            Ok(Some(d)) => d.segments,
            _ => return,
        };
        pending.retain(|s| s.log_seq.is_some_and(|q| q > covered));
        let (Some(first), Some(newest)) = (pending.first(), pending.last()) else { return };
        let window_ms = state.notes_window_mins.load(std::sync::atomic::Ordering::Relaxed) as i64 * 60_000;
        if !ended && newest.end_ms - first.start_ms < window_ms {
            return;
        }
        let seq = pending.iter().filter_map(|s| s.log_seq).max().unwrap_or(covered);
        let transcript = pending
            .iter()
            .map(|s| {
                let who = match s.source {
                    AudioSource::Mic => "You",
                    AudioSource::System => "Others",
                };
                format!("{who}: {}", s.text.trim())
            })
            .collect::<Vec<_>>()
            .join("\n");

        let tag = format!("notes #{seq} session={session_id}");
        let client = match configured_client(&app) {
            Ok(client) => client,
            Err(reason) => {
                log_step(log, &tag, &format!("{reason}; skipping"));
                let _ = app.emit(events::APP_STATUS, "Notes not generated: set a model and API key in Settings → LLM Configuration.");
                return;
            }
        };
        log_step(
            log,
            &tag,
            &format!("agent start (model={}, window={} chars)", client.model(), transcript.chars().count()),
        );

        let goal = state.goals.lock().unwrap().get(&session_id).cloned().unwrap_or_default();
        let existing = last.map(|n| n.text).unwrap_or_default();

        match agent_notes(&client, &transcript, &existing, &goal, ended, log) {
            Ok(text) => {
                log_step(log, &tag, &format!("final notes = {} chars", text.chars().count()));
                let _ = storage.save_note(session_id, seq, &text);
                let _ = storage.save_notes(session_id, &text);
                let _ = app.emit(events::NOTES, NotesUpdate { session_id, seq, text });
                if ended {
                    super::title::rename_from_notes(app, session_id);
                }
            }
            Err(e) => {
                eprintln!("[roofle] {tag}: {e}");
                let _ = app.emit(events::APP_STATUS, format!("Notes generation failed: {e}"));
            }
        }
    });
}
