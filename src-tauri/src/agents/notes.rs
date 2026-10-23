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
    run(app, session_id, ended, false);
}

/// Rewrite a finished session's notes from its whole transcript, ignoring the
/// notes saved so far. Like the end-of-call pass it ends with the wrap-up event.
pub fn regenerate(app: AppHandle, session_id: i64) {
    run(app, session_id, true, true);
}

fn run(app: AppHandle, session_id: i64, ended: bool, rebuild: bool) {
    std::thread::spawn(move || {
        let _run = RUN.lock().unwrap_or_else(|e| e.into_inner());
        let ready = update_notes(&app, session_id, ended, rebuild);
        if ended {
            super::wrap_up::finish(&app, session_id, ready);
        }
    });
}




/// One notes pass. `true` when the saved notes are current afterwards
/// (including "nothing new to add"). `rebuild` starts from an empty document and
/// the whole transcript instead of continuing from the last pass.
fn update_notes(app: &AppHandle, session_id: i64, ended: bool, rebuild: bool) -> bool {
    {
        let log = logging_enabled();
        let state = app.state::<AppState>();
        let storage = &state.storage;

        let last = storage.latest_note(session_id).ok().flatten();
        let covered = if rebuild { 0 } else { last.as_ref().map_or(0, |n| n.sequence_no) };
        let mut pending = match storage.get_session(session_id) {
            Ok(Some(d)) => d.segments,
            _ => return false,
        };
        pending.retain(|s| s.log_seq.is_some_and(|q| q > covered));
        let (Some(first), Some(newest)) = (pending.first(), pending.last()) else { return true };
        let window_ms = state.notes_window_mins.load(std::sync::atomic::Ordering::Relaxed) as i64 * 60_000;
        if !ended && newest.end_ms - first.start_ms < window_ms {
            return false;
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
        let client = match configured_client(app) {
            Ok(client) => client,
            Err(reason) => {
                log_step(log, &tag, &format!("{reason}; skipping"));
                let _ = app.emit(events::APP_STATUS, "Notes not generated: set a model and API key in Settings → LLM Configuration.");
                return false;
            }
        };
        log_step(
            log,
            &tag,
            &format!("agent start (model={}, window={} chars)", client.model(), transcript.chars().count()),
        );

        let goal = state.goals.lock().unwrap().get(&session_id).cloned().unwrap_or_default();
        let existing = if rebuild { String::new() } else { last.map(|n| n.text).unwrap_or_default() };

        match agent_notes(&client, &transcript, &existing, &goal, &crate::commands::persona::profile(storage).describe(""), ended, log) {
            Ok(text) => {
                log_step(log, &tag, &format!("final notes = {} chars", text.chars().count()));
                let _ = storage.save_note(session_id, seq, &text);
                let _ = storage.save_notes(session_id, &text);
                let _ = app.emit(events::NOTES, NotesUpdate { session_id, seq, text });
                if ended {
                    super::title::rename_from_notes(app.clone(), session_id);
                }
                true
            }
            Err(e) => {
                eprintln!("[roofle] {tag}: {e}");
                let _ = app.emit(events::APP_STATUS, format!("Notes generation failed: {e}"));
                false
            }
        }
    }
}
