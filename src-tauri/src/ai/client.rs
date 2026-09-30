use std::time::Duration;

use serde::Serialize;

use super::config::AiConfig;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
        }
    }
}

/// Client for any OpenAI-compatible `/chat/completions` endpoint.
pub struct AiClient {
    http: ureq::Agent,
}

impl AiClient {
    pub fn new() -> Self {
        Self {
            http: ureq::AgentBuilder::new().timeout(REQUEST_TIMEOUT).build(),
        }
    }

    pub fn chat(
        &self,
        config: &AiConfig,
        api_key: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<String, String> {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        let body = serde_json::json!({ "model": config.model, "messages": messages });

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

fn describe_http_error(url: &str, e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, response) => {
            let text = response.into_string().unwrap_or_default();
            format!("{url}: HTTP {code} {text}")
        }
        other => format!("{url}: {other}"),
    }
}
