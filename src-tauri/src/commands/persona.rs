//! The user's persona (job role and meeting types), chosen at first run and editable in Settings.
//! Stored in `app_config` so prompts and other features can read it later via [`load`].

use roofle_core::persona::PersonaProfile;
use live_agent::pipeline::MeetingType;
use roofle_core::storage::SqliteStorage;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

const KEY: &str = "persona";
const MAX_MEETINGS: usize = 16;
const MAX_SUMMARY: usize = 400;

/// `role_id` is None when the user skipped setup. Ids come from `src/data/personas.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Persona {
    pub role_id: Option<String>,
    pub meetings: Vec<MeetingType>,
    /// Editable summary shown at onboarding; empty means use the generated one.
    #[serde(default)]
    pub summary: String,
}

/// None until the user has answered or skipped setup.
pub fn load(storage: &SqliteStorage) -> Result<Option<Persona>, String> {
    let Some(raw) = storage.get_config(KEY).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    Ok(serde_json::from_str(&raw).ok())
}

/// Role and meeting labels for the saved persona (empty if none), as shown in Settings.
pub fn profile(storage: &SqliteStorage) -> PersonaProfile {
    let Ok(Some(p)) = load(storage) else { return PersonaProfile::default() };
    profile_of(&p)
}

fn profile_of(p: &Persona) -> PersonaProfile {
    let data: serde_json::Value = serde_json::from_str(include_str!("../../../src/data/personas.json")).unwrap_or_default();
    let role = data["roles"].as_array().and_then(|r| r.iter().find(|r| r["id"].as_str() == p.role_id.as_deref()));
    let label = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
    let meetings = p
        .meetings
        .iter()
        .filter_map(|m| {
            let ty = serde_json::to_value(m).ok()?;
            let found = role?["meetings"].as_array()?.iter().find(|x| x["type"] == ty)?;
            Some(label(&found["label"]))
        })
        .collect();
    let focus = role
        .and_then(|r| r["focus"].as_array())
        .map(|f| f.iter().map(label).collect())
        .unwrap_or_default();
    PersonaProfile { role: role.map(|r| label(&r["label"])).unwrap_or_default(), meetings, focus, summary: p.summary.clone() }
}

#[tauri::command]
pub fn get_persona(state: State<'_, AppState>) -> Result<Option<Persona>, String> {
    load(&state.storage)
}

/// The generated first-person summary for a role + meetings choice, to prefill the editable box.
#[tauri::command]
pub fn describe_persona(persona: Persona) -> String {
    PersonaProfile { summary: String::new(), ..profile_of(&persona) }.describe("")
}

#[tauri::command]
pub fn save_persona(state: State<'_, AppState>, persona: Persona) -> Result<(), String> {
    if persona.meetings.len() > MAX_MEETINGS || persona.role_id.as_deref().is_some_and(|r| r.len() > 64)
        || persona.summary.chars().count() > MAX_SUMMARY {
        return Err("invalid persona".into());
    }
    let raw = serde_json::to_string(&persona).map_err(|e| e.to_string())?;
    state.storage.set_config(KEY, &raw).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_uses_role_specific_labels() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        assert_eq!(profile(&storage), PersonaProfile::default());
        storage.set_config(KEY, r#"{"roleId":"engineering","meetings":["review","support"]}"#).unwrap();
        let p = profile(&storage);
        assert_eq!(p.role, "Engineer / Tech Lead");
        assert_eq!(p.meetings, vec!["Design & code reviews", "Incident & debugging calls"]);
        assert!(p.focus.contains(&"correctness and failure modes".to_string()));
    }

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
