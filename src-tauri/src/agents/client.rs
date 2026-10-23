use tauri::{AppHandle, Manager};

use crate::ai::{self, NotesClient};
use live_agent::pipeline::Stages;
use crate::state::AppState;

/// A completion client built from the saved config and the key captured at
/// recording start. `Err` carries the reason no client is available.
pub fn configured_client(app: &AppHandle) -> Result<NotesClient, String> {
    let state = app.state::<AppState>();
    // The recording-start key is empty for a finished session opened later; fall back to Settings.
    let captured = state.api_key.lock().unwrap().clone();
    let api_key = ai::resolve_api_key_with(&state.storage, &captured);
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

/// One client per pipeline stage, each on its own model.
pub struct StageClients {
    pub understand: NotesClient,
    pub write: NotesClient,
    pub judge: NotesClient,
    pub models: [String; 3],
}

impl StageClients {
    pub fn stages(&self) -> Stages<'_> {
        Stages { understand: &self.understand, write: &self.write, judge: &self.judge }
    }
}

pub fn stage_clients(app: &AppHandle) -> Result<StageClients, String> {
    let api_key = app.state::<AppState>().api_key.lock().unwrap().clone();
    let config = ai::load_for(app)?;
    if !config.is_usable_with(&api_key) {
        return Err(format!(
            "AI not configured (model={:?}, key={})",
            config.model,
            !api_key.trim().is_empty()
        ));
    }
    let make = |m: &str| NotesClient::new(config.for_stage(m), api_key.clone());
    let pick = |m: &str| if m.trim().is_empty() { config.model.clone() } else { m.trim().to_string() };
    Ok(StageClients {
        understand: make(&config.understand_model),
        write: make(&config.write_model),
        judge: make(&config.judge_model),
        models: [
            pick(&config.understand_model),
            pick(&config.write_model),
            pick(&config.judge_model),
        ],
    })
}
