use tauri::{AppHandle, Manager};

use crate::ai::{self, NotesClient};
use crate::state::AppState;

/// A completion client built from the saved config and the key captured at
/// recording start. `Err` carries the reason no client is available.
pub fn configured_client(app: &AppHandle) -> Result<NotesClient, String> {
    let api_key = app.state::<AppState>().api_key.lock().unwrap().clone();
    let config = ai::load_for(app)?;
    if !config.is_usable_with(&api_key) {
        return Err(format!(
            "AI not configured (model={:?}, key={})",
            config.model,
            !api_key.trim().is_empty()
        ));
    }
    Ok(NotesClient::new(config, api_key))
}
