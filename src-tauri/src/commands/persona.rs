//! The user's persona (job role and meeting types), chosen at first run and editable in Settings.
//! Stored in `app_config` so prompts and other features can read it later via [`load`].

use live_agent::pipeline::MeetingType;
use roofle_core::storage::SqliteStorage;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

const KEY: &str = "persona";
const MAX_MEETINGS: usize = 16;

/// `role_id` is None when the user skipped setup. Ids come from `src/data/personas.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Persona {
    pub role_id: Option<String>,
    pub meetings: Vec<MeetingType>,
}

/// None until the user has answered or skipped setup.
pub fn load(storage: &SqliteStorage) -> Result<Option<Persona>, String> {
    let Some(raw) = storage.get_config(KEY).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    Ok(serde_json::from_str(&raw).ok())
}

#[tauri::command]
pub fn get_persona(state: State<'_, AppState>) -> Result<Option<Persona>, String> {
    load(&state.storage)
}

#[tauri::command]
pub fn save_persona(state: State<'_, AppState>, persona: Persona) -> Result<(), String> {
    if persona.meetings.len() > MAX_MEETINGS || persona.role_id.as_deref().is_some_and(|r| r.len() > 64) {
        return Err("invalid persona".into());
    }
    let raw = serde_json::to_string(&persona).map_err(|e| e.to_string())?;
    state.storage.set_config(KEY, &raw).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_json_shape() {
        let p: Persona = serde_json::from_str(r#"{"roleId":"sales","meetings":["sales","discovery"]}"#).unwrap();
        assert_eq!(p.role_id.as_deref(), Some("sales"));
        assert_eq!(p.meetings, vec![MeetingType::Sales, MeetingType::Discovery]);
        assert!(serde_json::from_str::<Persona>(r#"{"roleId":null,"meetings":["nope"]}"#).is_err());
    }

    /// Every meeting type in the shipped JSON must be one the backend knows.
    #[test]
    fn shipped_personas_use_known_types() {
        let data: serde_json::Value = serde_json::from_str(include_str!("../../../src/data/personas.json")).unwrap();
        for role in data["roles"].as_array().unwrap() {
            for m in role["meetings"].as_array().unwrap() {
                let t = m["type"].clone();
                serde_json::from_value::<MeetingType>(t.clone()).unwrap_or_else(|_| panic!("unknown type {t}"));
            }
        }
    }
}
