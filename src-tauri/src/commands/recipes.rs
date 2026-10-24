use offline_agent::{parse_action_items, recipes, ActionItem, Draft, Recipe};
use roofle_core::storage::StoredRecipe;
use tauri::{AppHandle, State};

use crate::agents::wrap_up::session_notes;
use crate::ai::{self, NotesClient};
use crate::state::AppState;

const NOT_CONFIGURED: &str = "AI not configured: set a model and API key in Settings";

/// Owners, tasks and due dates from a session's notes. No LLM involved.
#[tauri::command]
pub fn get_action_items(app: AppHandle, id: i64) -> Vec<ActionItem> {
    parse_action_items(&session_notes(&app, id))
}

/// Write a finished session's notes again from its whole transcript. Returns at
/// once; the notes arrive as `notes-update`, then `call-wrapped-up`.
#[tauri::command]
pub fn regenerate_notes(app: AppHandle, id: i64) {
    crate::agents::notes::regenerate(app, id);
}

/// A draft generated earlier, so reopening a recipe never calls the LLM.
#[tauri::command]
pub fn get_recipe(state: State<'_, AppState>, id: i64, recipe: Recipe) -> Result<Option<Draft>, String> {
    let stored = state.storage.load_recipe(id, recipe.key()).map_err(|e| e.to_string())?;
    Ok(stored.map(|StoredRecipe { subject, body }| Draft { subject, body }))
}

/// Draft a recap email or Slack post from a session's notes and store it.
/// Called for the first generation and for an explicit regenerate.
#[tauri::command]
pub async fn run_recipe(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    recipe: Recipe,
    api_key: String,
) -> Result<Draft, String> {
    let config = ai::load_for(&app)?;
    let api_key = ai::resolve_api_key_with(&state.storage, &api_key);
    if !config.is_usable_with(&api_key) {
        return Err(NOT_CONFIGURED.to_string());
    }
    let notes = session_notes(&app, id);
    let goal = state.goals.lock().unwrap().get(&id).cloned().unwrap_or_default();

    let app_handle = app.clone();
    let draft = tauri::async_runtime::spawn_blocking(move || {
        recipes::run(&NotesClient::new(config, api_key).with_usage(&app_handle, id), recipe, &notes, &goal)
    })
    .await
    .map_err(|e| format!("task failed: {e}"))??;

    state
        .storage
        .save_recipe(id, recipe.key(), draft.subject.as_deref(), &draft.body)
        .map_err(|e| e.to_string())?;
    Ok(draft)
}
