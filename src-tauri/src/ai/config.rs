use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const CONFIG_FILE: &str = "ai-config.json";
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    OpenAiCompat,
}

/// Non-secret settings. The API key is deliberately absent: it lives in the
/// renderer's localStorage and is supplied per call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub kind: ProviderKind,
    pub base_url: String,
    pub model: String,
    /// Optional per-stage models for the question pipeline; empty = `model`.
    #[serde(default)]
    pub understand_model: String,
    #[serde(default)]
    pub write_model: String,
    #[serde(default)]
    pub judge_model: String,
}

impl AiConfig {
    /// A copy of this config using `stage_model` when set, else the main model.
    pub fn for_stage(&self, stage_model: &str) -> AiConfig {
        let mut c = self.clone();
        if !stage_model.trim().is_empty() {
            c.model = stage_model.trim().to_string();
        }
        c
    }

    pub fn is_usable_with(&self, api_key: &str) -> bool {
        !self.model.trim().is_empty() && !api_key.trim().is_empty()
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            kind: ProviderKind::OpenAiCompat,
            base_url: DEFAULT_BASE_URL.to_string(),
            model: String::new(),
            understand_model: String::new(),
            write_model: String::new(),
            judge_model: String::new(),
        }
    }
}

/// Config as shown to the renderer; `has_api_key` reflects env vars only.
#[derive(Debug, Clone, Serialize)]
pub struct AiConfigView {
    pub kind: ProviderKind,
    pub base_url: String,
    pub model: String,
    pub has_api_key: bool,
}

pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("app data dir: {e}"))
}

/// The persisted config, or the default when missing or unreadable.
pub fn load(app_data_dir: &Path) -> AiConfig {
    std::fs::read_to_string(app_data_dir.join(CONFIG_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Load the config from the app's data dir.
pub fn load_for(app: &AppHandle) -> Result<AiConfig, String> {
    Ok(load(&data_dir(app)?))
}

pub fn save(app_data_dir: &Path, config: &AiConfig) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(app_data_dir).map_err(|e| format!("create data dir: {e}"))?;
    std::fs::write(app_data_dir.join(CONFIG_FILE), raw).map_err(|e| format!("write config: {e}"))
}
