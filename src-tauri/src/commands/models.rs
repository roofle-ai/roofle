use tauri::{AppHandle, Manager, State};

use crate::models::ModelInfo;
use crate::state::AppState;

#[tauri::command]
pub fn list_models(state: State<AppState>) -> Vec<ModelInfo> {
    state.models.list()
}

#[tauri::command]
pub fn select_model(state: State<AppState>, id: String) -> Result<(), String> {
    if !state.models.list().iter().any(|m| m.id == id) {
        return Err(format!("unknown model: {id}"));
    }
    *state.selected_model.lock().unwrap() = id;
    Ok(())
}

#[tauri::command]
pub fn selected_model(state: State<AppState>) -> String {
    state.selected_model.lock().unwrap().clone()
}

/// Whether the selected model is on disk; the UI gates recording on this.
#[tauri::command]
pub fn models_ready(state: State<AppState>) -> bool {
    let id = state.selected_model.lock().unwrap().clone();
    state.models.ready(&id)
}

/// Download the selected model and VAD (progress arrives as `model-progress`).
#[tauri::command]
pub async fn download_model(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let id = state.selected_model.lock().unwrap().clone();
        state.models.ensure(&app, &id)?;
        state.models.ensure_vad(&app).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn cancel_model_download(state: State<AppState>) {
    state.models.cancel_download();
}
