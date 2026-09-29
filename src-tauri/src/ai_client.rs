// roofle-app — shared LLM completion client for the agents.
//
// All offline/live agents talk to the model through [`CompletionClient`].
// `NotesClient` adapts the app's OpenAI-compatible client to that trait, so
// agent logic stays pure (in live-agent / offline-agent crates) and this shell
// only supplies the HTTP + config plumbing.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use live_agent::notes::{logging_enabled, CompletionClient};
use tauri::{AppHandle, Manager};

use crate::ai::{AiClient, AiConfig, ChatMessage};

/// Resolve the app data dir for AI config persistence.
pub fn ai_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("app data dir: {e}"))
}

/// A `CompletionClient` backed by the configured OpenAI-compatible AI client,
/// so the agents can call the LLM through the app's abstraction.
pub struct NotesClient {
    config: AiConfig,
    api_key: String,
}

impl NotesClient {
    pub fn new(config: AiConfig, api_key: String) -> Self {
        Self { config, api_key }
    }
}

impl CompletionClient for NotesClient {
    fn complete(&self, system: &str, user: &str) -> Result<String, String> {
        let result = AiClient::new().chat(
            &self.config,
            &self.api_key,
            vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user.to_string(),
                },
            ],
        );
        log_llm_exchange(system, user, &result);
        result
    }
}

/// Disposable dev aid: when `ROOFLE_LOG_AI` is on, append the exact request
/// (system + user) and the LLM response to a timestamped text file in the
/// project root. Remove this together with its caller when no longer needed.
fn log_llm_exchange(system: &str, user: &str, result: &Result<String, String>) {
    if !logging_enabled() {
        return;
    }
    // One log file per run, named by the millisecond timestamp of the first call.
    static LOG_FILE: OnceLock<PathBuf> = OnceLock::new();
    let file = LOG_FILE.get_or_init(|| {
        // Project root = parent of the src-tauri crate dir (compile-time).
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or(Path::new("."));
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        root.join(format!("ai-llm-{stamp}.log"))
    });

    let stamp = Utc::now().to_rfc3339();
    let block = match result {
        Ok(reply) => format!(
            "== {stamp} ==\n--- SYSTEM ---\n{system}\n--- USER ---\n{user}\n--- REPLY ---\n{reply}\n\n"
        ),
        Err(err) => format!(
            "== {stamp} ==\n--- SYSTEM ---\n{system}\n--- USER ---\n{user}\n--- ERROR ---\n{err}\n\n"
        ),
    };
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .and_then(|mut f| f.write_all(block.as_bytes()));
}
