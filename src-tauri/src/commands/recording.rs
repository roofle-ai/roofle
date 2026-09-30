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
    if !source.is_implemented() {
        return Err(format!("source {source:?} is not implemented yet"));
    }
    *state.last_source.lock().unwrap() = source;
    // A tray start has no renderer key; keep the last one a popup start set.
    let resolved = ai::resolve_api_key(&api_key);
    if !resolved.is_empty() {
        *state.api_key.lock().unwrap() = resolved;
    }
    request(&state, |reply| Control::Start { source, session_id, reply })?;
    set_recording_state(&app, true);
    Ok(())
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
