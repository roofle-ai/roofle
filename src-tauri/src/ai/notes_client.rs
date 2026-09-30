use chrono::Utc;
use live_agent::notes::{logging_enabled, CompletionClient};

use super::client::{AiClient, ChatMessage};
use super::config::AiConfig;
use crate::dev_log::RunLog;

static LLM_LOG: RunLog = RunLog::new("ai-llm");

/// Adapts [`AiClient`] to the agents' [`CompletionClient`] trait.
pub struct NotesClient {
    config: AiConfig,
    api_key: String,
}

impl NotesClient {
    pub fn new(config: AiConfig, api_key: String) -> Self {
        Self { config, api_key }
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
        let result = AiClient::new().chat(&self.config, &self.api_key, messages);
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
