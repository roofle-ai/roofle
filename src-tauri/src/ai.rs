// roofle-app — reusable OpenAI-compatible AI client (Slice 1).
//
// One client covers the providers that expose an OpenAI-compatible
// `/chat/completions` endpoint: OpenAI, OpenRouter, Groq, Together, Azure,
// Ollama, LM Studio, llama.cpp. Future features (follow-up questions,
// summaries, …) share this client instead of owning their own HTTP.
//
// The provider/model/base URL are chosen in App Settings and persisted as JSON
// in the app data dir. The API key lives in the renderer's localStorage (kept
// out of the OS keychain) and is handed back to Rust per call; Rust never
// persists it. For local/dev runs the key can instead come from an env var
// (see KEY_ENV_VARS), which takes precedence over (and bypasses) the
// renderer-supplied key.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Providers backed by an OpenAI-compatible HTTP API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// OpenAI-compatible `/chat/completions` (OpenAI, OpenRouter, Groq, Ollama…).
    OpenAiCompat,
}

/// Non-secret AI configuration persisted to `ai-config.json` (app data dir).
/// The API key is deliberately absent: it lives in the renderer's localStorage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub kind: ProviderKind,
    /// Provider base URL, e.g. `https://api.openai.com/v1` or
    /// `http://127.0.0.1:11434/v1` (Ollama). Trailing slash optional.
    pub base_url: String,
    /// Model identifier, e.g. `gpt-4o` or `llama3.1`.
    pub model: String,
}

/// Config view returned to the renderer: no API key. `has_api_key` reflects
/// the local key env vars (see [`KEY_ENV_VARS`]) only; the renderer decides
/// presence from its own localStorage (which now owns the key).
#[derive(Debug, Clone, Serialize)]
pub struct AiConfigView {
    pub kind: ProviderKind,
    pub base_url: String,
    pub model: String,
    pub has_api_key: bool,
}

/// One message in an OpenAI-compatible chat request.
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Env vars supplying the API key for local/dev runs, checked in order
/// (bypass the renderer-supplied key). `ROOFLE_LOCAL_KEY` is roofle's own;
/// the rest are common provider conventions so a tray start without a
/// renderer-supplied key still works.
const KEY_ENV_VARS: &[&str] = &["ROOFLE_LOCAL_KEY", "OPENROUTER_API_KEY", "OPENAI_API_KEY"];
/// Config filename inside the app data dir.
const CONFIG_FILE: &str = "ai-config.json";

fn config_path(app_data_dir: &Path) -> std::path::PathBuf {
    app_data_dir.join(CONFIG_FILE)
}

/// Default config: OpenAI's v1 endpoint, empty model (unset until configured).
fn default_config() -> AiConfig {
    AiConfig {
        kind: ProviderKind::OpenAiCompat,
        base_url: "https://api.openai.com/v1".to_string(),
        model: String::new(),
    }
}

/// Read persisted config; returns the default when missing or unreadable.
pub fn load_config(app_data_dir: &Path) -> AiConfig {
    std::fs::read_to_string(config_path(app_data_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(default_config)
}

/// Persist the non-secret config to the app data dir.
pub fn save_config(app_data_dir: &Path, config: &AiConfig) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(app_data_dir).map_err(|e| format!("create data dir: {e}"))?;
    std::fs::write(config_path(app_data_dir), raw).map_err(|e| format!("write config: {e}"))
}

/// The API key supplied by the first non-empty local key env var (see
/// [`KEY_ENV_VARS`]).
pub(crate) fn env_api_key() -> Option<String> {
    KEY_ENV_VARS
        .iter()
        .find_map(|name| {
            std::env::var(name)
                .ok()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
        })
}

/// Resolve the effective API key for a request: a local key env var (see
/// [`KEY_ENV_VARS`]) wins over the renderer-supplied key (from localStorage).
/// Empty when neither is available. Rust never persists the key.
pub fn resolve_api_key(renderer_key: &str) -> String {
    if let Some(key) = env_api_key() {
        return key;
    }
    renderer_key.trim().to_string()
}

/// A reusable OpenAI-compatible chat client.
pub struct AiClient {
    http: ureq::Agent,
}

impl AiClient {
    pub fn new() -> Self {
        Self {
            http: ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(90))
                .build(),
        }
    }

    /// POST `/chat/completions` and return the assistant's reply text.
    /// Any provider speaking the OpenAI wire format is supported.
    pub fn chat(
        &self,
        config: &AiConfig,
        api_key: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<String, String> {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": config.model,
            "messages": messages,
        });

        let mut request = self.http.post(&url);
        if !api_key.is_empty() {
            request = request.set("Authorization", &format!("Bearer {api_key}"));
        }
        let response = request
            .send_json(body)
            .map_err(|e| describe_http_error(&url, e))?;
        let status = response.status();
        if !(200..300).contains(&status) {
            let text = response.into_string().unwrap_or_default();
            return Err(format!("HTTP {status}: {text}"));
        }
        let json: serde_json::Value = response
            .into_json()
            .map_err(|e| format!("parse response: {e}"))?;
        json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.trim().to_string())
            .ok_or_else(|| "no content in model response".to_string())
    }
}

impl Default for AiClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Render a ureq transport error for the user; keeps the message short.
fn describe_http_error(url: &str, e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, response) => {
            let text = response.into_string().unwrap_or_default();
            format!("{url}: HTTP {code} {text}")
        }
        other => format!("{url}: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests mutate the process env; serialize them to avoid races.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn env_key_falls_back_to_provider_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("ROOFLE_LOCAL_KEY");
        std::env::remove_var("OPENROUTER_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");

        // Tray start passes no renderer key; a provider env var must suffice.
        std::env::set_var("OPENROUTER_API_KEY", "sk-openrouter");
        assert_eq!(resolve_api_key(""), "sk-openrouter");

        // ROOFLE_LOCAL_KEY keeps documented precedence over provider vars.
        std::env::set_var("ROOFLE_LOCAL_KEY", "sk-local");
        assert_eq!(resolve_api_key(""), "sk-local");
    }

    #[test]
    fn renderer_key_used_when_no_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("ROOFLE_LOCAL_KEY");
        std::env::remove_var("OPENROUTER_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");

        assert_eq!(resolve_api_key("  sk-renderer  "), "sk-renderer");
    }
}
