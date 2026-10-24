use chrono::Utc;
use live_agent::notes::{logging_enabled, CompletionClient};

use tauri::{AppHandle, Manager};

use super::client::{AiClient, ChatMessage, Usage};
use super::config::AiConfig;
use crate::dev_log::RunLog;
use crate::state::AppState;

static LLM_LOG: RunLog = RunLog::new("ai-llm");

/// Adapts [`AiClient`] to the agents' [`CompletionClient`] trait.
pub struct NotesClient {
    config: AiConfig,
    api_key: String,
    /// Where each call's token usage is recorded; `None` records nothing.
    usage_to: Option<(AppHandle, i64)>,
}

impl NotesClient {
    pub fn new(config: AiConfig, api_key: String) -> Self {
        Self { config, api_key, usage_to: None }
    }

    /// Record every call's tokens against `session_id`.
    pub fn with_usage(mut self, app: &AppHandle, session_id: i64) -> Self {
        self.usage_to = Some((app.clone(), session_id));
        self
    }

    fn record(&self, usage: &Usage) {
        let Some((app, session_id)) = &self.usage_to else { return };
        let storage = &app.state::<AppState>().storage;
        let _ = storage.record_usage(Some(*session_id), &self.config.model, usage.tokens_in, usage.tokens_out);
    }

    pub fn model(&self) -> &str {
        &self.config.model
    }
}

impl CompletionClient for NotesClient {
    fn complete(&self, system: &str, user: &str) -> Result<String, String> {
        let messages = vec![
            ChatMessage::new("system", system),
            ChatMessage::new("user", user),
        ];
        let result = AiClient::new()
            .chat(&self.config, &self.api_key, messages)
            .map(|(text, usage)| {
                self.record(&usage);
                text
            });
        if logging_enabled() {
            log_exchange(system, user, &result);
        }
        result
    }

    fn complete_json(
        &self,
        system: &str,
        user: &str,
        schema: &serde_json::Value,
    ) -> Result<String, String> {
        let messages = vec![
            ChatMessage::new("system", system),
            ChatMessage::new("user", user),
        ];
        let result = AiClient::new().chat_json(&self.config, &self.api_key, messages, schema);
        let result = result.map(|(text, usage)| {
            self.record(&usage);
            if logging_enabled() {
                eprintln!(
                    "[roofle] llm tokens in={} out={} latency={}ms",
                    usage.tokens_in, usage.tokens_out, usage.latency_ms
                );
            }
            text
        });
        if logging_enabled() {
            log_exchange(system, user, &result);
        }
        result
    }
}

fn log_exchange(system: &str, user: &str, result: &Result<String, String>) {
    let stamp = Utc::now().to_rfc3339();
    let (label, outcome) = match result {
        Ok(reply) => ("REPLY", reply),
        Err(err) => ("ERROR", err),
    };
    LLM_LOG.append(&format!(
        "== {stamp} ==\n--- SYSTEM ---\n{system}\n--- USER ---\n{user}\n--- {label} ---\n{outcome}\n\n"
    ));
}
