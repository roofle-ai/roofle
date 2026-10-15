use live_agent::notes::{log_step, logging_enabled};
use live_agent::figures::{extract_figures, KeyFigures};
use live_agent::recap::{recap_recent, Recap, Utterance};
use offline_agent::analysis::{analyse, AnalysisResult};
use roofle_core::storage::{AudioSegment, AudioSource, SessionRepository, SessionStatus};
use tauri::{AppHandle, Manager, State};

use crate::ai::{self, AiClient, AiConfig, AiConfigView, ChatMessage, NotesClient, ProviderKind};
use crate::state::AppState;

const NOT_CONFIGURED: &str = "AI not configured: set a model and API key in Settings";

/// Never returns the API key; `has_api_key` reflects env vars or the saved key.
#[tauri::command]
pub fn load_ai_config(app: AppHandle, state: State<AppState>) -> Result<AiConfigView, String> {
    let config = ai::load_for(&app)?;
    Ok(AiConfigView {
        kind: config.kind,
        base_url: config.base_url,
        model: config.model,
        has_api_key: !ai::resolve_api_key_with(&state.storage, "").is_empty(),
    })
}

#[tauri::command]
pub fn save_ai_config(
    app: AppHandle,
    state: State<AppState>,
    base_url: String,
    model: String,
    api_key: Option<String>,
) -> Result<(), String> {
    // Blank or absent means "unchanged".
    if let Some(key) = api_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        state.storage.set_config(ai::KEY_SETTING, key).map_err(|e| e.to_string())?;
    }
    // Keep the per-stage models, which this form does not edit.
    let config = AiConfig {
        kind: ProviderKind::OpenAiCompat,
        base_url,
        model,
        ..ai::load_for(&app)?
    };
    ai::save(&ai::data_dir(&app)?, &config)
}

/// Round-trip a trivial prompt to verify the settings end to end.
#[tauri::command]
pub async fn test_ai_connection(app: AppHandle, api_key: String) -> Result<String, String> {
    let config = ai::load_for(&app)?;
    let api_key = ai::resolve_api_key_with(&app.state::<AppState>().storage, &api_key);
    tauri::async_runtime::spawn_blocking(move || {
        let messages = vec![ChatMessage::new("user", "Reply with the single word OK.")];
        AiClient::new().chat(&config, &api_key, messages)
    })
    .await
    .map_err(|e| format!("task failed: {e}"))?
    .map_err(|e| friendly_error(&e))
}

/// One readable line for the settings screen; the raw error stays in the logs.
fn friendly_error(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let code = raw
        .split("HTTP ")
        .nth(1)
        .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|c| c.parse::<u16>().ok());
    let msg = match code {
        Some(401 | 403) => "The API key was rejected. Check the key and try again.".to_string(),
        Some(404) => "Endpoint or model not found. Check the base URL and model name.".to_string(),
        Some(429) => "Rate limit or quota reached. Try again shortly or check your plan.".to_string(),
        Some(c) if c >= 500 => "The provider had a server error. Try again in a moment.".to_string(),
        Some(c) => format!("The provider rejected the request (HTTP {c})."),
        None if lower.contains("timed out") => "The request timed out. Check the base URL and your network.".to_string(),
        None if lower.contains("dns") || lower.contains("connection") => {
            "Could not reach the server. Check the base URL and your network.".to_string()
        }
        None if lower.contains("not configured") || lower.contains("base url") || lower.contains("model") => {
            raw.lines().next().unwrap_or(raw).chars().take(120).collect()
        }
        None => "Connection test failed. Check the base URL, model and API key.".to_string(),
    };
    msg
}

/// Evaluate a finished session and persist the result so a later click shows
/// it without re-analysing.
#[tauri::command]
pub async fn analyse_session(
    app: AppHandle,
    id: i64,
    context: String,
    api_key: String,
) -> Result<AnalysisResult, String> {
    let config = ai::load_for(&app)?;
    let api_key = ai::resolve_api_key_with(&app.state::<AppState>().storage, &api_key);
    if !config.is_usable_with(&api_key) {
        return Err(NOT_CONFIGURED.to_string());
    }

    let storage = &app.state::<AppState>().storage;
    let transcript = storage
        .get_session(id)
        .map_err(|e| e.to_string())?
        .map(|d| d.transcript)
        .unwrap_or_default();
    if transcript.trim().is_empty() {
        return Err("this session has no transcript to analyse".to_string());
    }

    let log = logging_enabled();
    log_step(
        log,
        "offline/analyse",
        &format!(
            "session={id} model={} context={context} transcript={} chars",
            config.model,
            transcript.chars().count(),
        ),
    );

    let stored_context = context.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let client = NotesClient::new(config, api_key);
        analyse(&client, &transcript, &context)
    })
    .await
    .map_err(|e| format!("task failed: {e}"))??;

    log_step(
        log,
        "offline/analyse",
        &format!(
            "parsed session={id} dimensions={} caveats={} overall={} lowest={:?} summary={:?}",
            result.dimensions.len(),
            result.caveats.len(),
            result.overall_score,
            result.lowest_scoring_dimension,
            result.call_summary,
        ),
    );

    let json = serde_json::to_string(&result).map_err(|e| e.to_string())?;
    storage
        .save_analysis(id, &stored_context, &json)
        .map_err(|e| e.to_string())?;
    Ok(result)
}

#[derive(serde::Serialize)]
pub struct AnalysisView {
    context: String,
    result: AnalysisResult,
}

/// The persisted analysis, if any; the renderer shows it instead of re-running.
#[tauri::command]
pub fn get_session_analysis(app: AppHandle, id: i64) -> Result<Option<AnalysisView>, String> {
    let stored = app
        .state::<AppState>()
        .storage
        .load_analysis(id)
        .map_err(|e| e.to_string())?;
    stored
        .map(|s| {
            let result = serde_json::from_str(&s.result_json)
                .map_err(|e| format!("stored analysis parse: {e}"))?;
            Ok(AnalysisView { context: s.context, result })
        })
        .transpose()
}

/// The key the live agents use (captured at recording start); the renderer's
/// key only covers sessions viewed without a recording this run.
fn live_or_given_key(app: &AppHandle, api_key: &str) -> String {
    let state = app.state::<AppState>();
    let live_key = state.api_key.lock().unwrap().clone();
    if live_key.trim().is_empty() { ai::resolve_api_key_with(&state.storage, api_key) } else { live_key }
}

/// Speaker-tagged utterances for the recap-style agents.
fn utterances(segments: Vec<AudioSegment>) -> Vec<Utterance> {
    segments
        .into_iter()
        .map(|s| Utterance {
            speaker: match s.source {
                AudioSource::Mic => "You",
                AudioSource::System => "Others",
            },
            text: s.text,
            end_ms: s.end_ms,
        })
        .collect()
}

/// Recap the last `window_secs` of a session (live or finished). `None` when
/// nothing has been said yet.
#[tauri::command]
pub async fn recap_session(
    app: AppHandle,
    id: i64,
    window_secs: u32,
    api_key: String,
) -> Result<Option<Recap>, String> {
    let config = ai::load_for(&app)?;
    let api_key = live_or_given_key(&app, &api_key);
    if !config.is_usable_with(&api_key) {
        return Err(NOT_CONFIGURED.to_string());
    }

    let segments = app
        .state::<AppState>()
        .storage
        .get_session(id)
        .map_err(|e| e.to_string())?
        .map(|d| d.segments)
        .unwrap_or_default();
    let utterances = utterances(segments);

    tauri::async_runtime::spawn_blocking(move || {
        let client = NotesClient::new(config, api_key);
        recap_recent(&client, &utterances, window_secs)
    })
    .await
    .map_err(|e| format!("task failed: {e}"))?
}

/// Storage key for a finished session's cached key figures.
const FIGURES_KEY: &str = "key-figures";

/// Key dates, numbers and names from a session. A finished session's figures
/// are stored on first use and reused afterwards (no LLM call); a live session
/// is always extracted fresh, and `refresh` forces a new extraction.
/// `None` when nothing has been said yet.
#[tauri::command]
pub async fn key_figures(
    app: AppHandle,
    id: i64,
    refresh: bool,
    api_key: String,
) -> Result<Option<KeyFigures>, String> {
    let state = app.state::<AppState>();
    let detail = state.storage.get_session(id).map_err(|e| e.to_string())?;
    let Some(detail) = detail else { return Ok(None) };
    let finished = detail.session.status == SessionStatus::Completed;

    if finished && !refresh {
        let stored = state.storage.load_recipe(id, FIGURES_KEY).map_err(|e| e.to_string())?;
        if let Some(figures) = stored.and_then(|s| serde_json::from_str(&s.body).ok()) {
            return Ok(Some(figures));
        }
    }

    let config = ai::load_for(&app)?;
    let api_key = live_or_given_key(&app, &api_key);
    if !config.is_usable_with(&api_key) {
        return Err(NOT_CONFIGURED.to_string());
    }
    let utterances = utterances(detail.segments);
    let figures = tauri::async_runtime::spawn_blocking(move || {
        extract_figures(&NotesClient::new(config, api_key), &utterances)
    })
    .await
    .map_err(|e| format!("task failed: {e}"))??;

    if let (true, Some(figures)) = (finished, &figures) {
        let body = serde_json::to_string(figures).map_err(|e| e.to_string())?;
        state.storage.save_recipe(id, FIGURES_KEY, None, &body).map_err(|e| e.to_string())?;
    }
    Ok(figures)
}

#[cfg(test)]
mod tests {
    use super::friendly_error;

    #[test]
    fn hides_raw_http_bodies() {
        let raw = "https://x/v1/chat/completions: HTTP 401 {\"error\":{\"message\":\"Incorrect API key sk-abc\"}}";
        let m = friendly_error(raw);
        assert!(m.contains("API key") && !m.contains("sk-abc") && !m.contains("http"));
        assert!(friendly_error("u: Connection Failed: dns error").contains("reach"));
    }
}
