use std::time::Duration;

use serde::Serialize;

use super::config::AiConfig;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
/// Pipeline stages are small, single-purpose calls; fail fast and skip the chunk.
const JSON_TIMEOUT: Duration = Duration::from_secs(20);

/// Cost and latency of one structured call.
#[derive(Debug, Clone, Copy, Default)]
pub struct Usage {
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub latency_ms: u64,
}

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
        let body = serde_json::json!({ "model": config.model, "messages": messages });
        self.post(&self.http, config, api_key, body).map(|(text, _)| text)
    }

    /// Structured call: `temperature` 0 and a strict JSON schema, so the reply
    /// always parses. One jittered retry on 429, 5xx or a transport error;
    /// never on a bad reply (the caller skips the chunk).
    pub fn chat_json(
        &self,
        config: &AiConfig,
        api_key: &str,
        messages: Vec<ChatMessage>,
        schema: &serde_json::Value,
    ) -> Result<(String, Usage), String> {
        let body = serde_json::json!({
            "model": config.model,
            "messages": messages,
            "temperature": 0,
            "response_format": {
                "type": "json_schema",
                "json_schema": { "name": "stage_output", "strict": true, "schema": schema },
            },
        });
        let http = ureq::AgentBuilder::new().timeout(JSON_TIMEOUT).build();
        let first = self.post(&http, config, api_key, body.clone());
        match first {
            Err(e) if is_retryable(&e) => {
                std::thread::sleep(Duration::from_millis(500 + jitter_ms(500)));
                self.post(&http, config, api_key, body)
            }
            other => other,
        }
    }

    fn post(
        &self,
        http: &ureq::Agent,
        config: &AiConfig,
        api_key: &str,
        body: serde_json::Value,
    ) -> Result<(String, Usage), String> {
        let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
        let started = std::time::Instant::now();

        let mut request = http.post(&url);
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
        let usage = Usage {
            tokens_in: json["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            tokens_out: json["usage"]["completion_tokens"].as_u64().unwrap_or(0),
            latency_ms: started.elapsed().as_millis() as u64,
        };
        json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| (s.trim().to_string(), usage))
            .ok_or_else(|| "no content in model response".to_string())
    }
}

impl Default for AiClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Rate limits, server errors and timeouts are worth one more try.
fn is_retryable(e: &str) -> bool {
    e.contains("HTTP 429") || e.contains("HTTP 5") || e.contains("timed out") || e.contains("Connection")
}

/// Cheap jitter without a rand dependency.
fn jitter_ms(max: u32) -> u64 {
    (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos())
        % max) as u64
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
