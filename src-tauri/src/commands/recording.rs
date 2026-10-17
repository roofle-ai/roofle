use std::sync::mpsc;

use tauri::{AppHandle, State};

use crate::ai;
use crate::app::tray::set_recording_state;
use crate::engine::{Control, SourceKind};
use crate::state::AppState;

/// Async so a first-run model download does not block the UI thread.
#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    source: SourceKind,
    session_id: Option<i64>,
    api_key: String,
) -> Result<(), String> {
    crate::license::require(&state.storage)?;
    if !source.is_implemented() {
        return Err(format!("source {source:?} is not implemented yet"));
    }
    *state.last_source.lock().unwrap() = source;
    // A tray start has no renderer key; keep the last one a popup start set.
    let resolved = ai::resolve_api_key_with(&state.storage, &api_key);
    if !resolved.is_empty() {
        *state.api_key.lock().unwrap() = resolved;
    }
    request(&state, |reply| Control::Start { source, session_id, reply })?;
    set_recording_state(&app, true);
    Ok(())
}

/// Turn the follow-up questions agent on or off; while off, no LLM calls are made for it.
#[tauri::command]
pub fn set_followups_enabled(state: State<'_, AppState>, enabled: bool) {
    state.followups_enabled.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// Paragraph trigger from settings; read when the next session begins.
#[tauri::command]
pub fn set_char_limit(state: State<'_, AppState>, limit: usize) {
    state.char_limit.store(limit, std::sync::atomic::Ordering::Relaxed);
}

/// How many follow-up questions may be open at once; clamped to 1..=5.
#[tauri::command]
pub fn set_max_open_questions(state: State<'_, AppState>, limit: usize) {
    let limit = limit.clamp(1, live_agent::pipeline::config::MAX_OPEN_CAP);
    state.max_open_questions.store(limit, std::sync::atomic::Ordering::Relaxed);
}

/// Minutes of transcript between note updates; clamped to 1..=60.
#[tauri::command]
pub fn set_notes_window_mins(state: State<'_, AppState>, mins: usize) {
    state.notes_window_mins.store(mins.clamp(1, 60), std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    request(&state, |reply| Control::Stop { reply })?;
    set_recording_state(&app, false);
    Ok(())
}

#[tauri::command]
pub async fn pause_recording(state: State<'_, AppState>) -> Result<(), String> {
    request(&state, |reply| Control::Pause { reply })
}

#[tauri::command]
pub async fn resume_recording(state: State<'_, AppState>) -> Result<(), String> {
    request(&state, |reply| Control::Resume { reply })
}

/// Send a control message to the engine thread and block for its reply.
fn request(
    state: &AppState,
    make: impl FnOnce(crate::engine::Reply) -> Control,
) -> Result<(), String> {
    let tx = state
        .engine
        .lock()
        .unwrap()
        .clone()
        .ok_or("engine is not running")?;
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(make(reply_tx)).map_err(|_| "engine shut down")?;
    reply_rx.recv().map_err(|_| "engine shut down")?
}
