use tauri::State;

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
