use live_agent::notes::{log_step, logging_enabled};
use offline_agent::analysis::{analyse, AnalysisResult};
use roofle_core::storage::SessionRepository;
use tauri::{AppHandle, Manager};

use crate::ai::{self, AiClient, AiConfig, AiConfigView, ChatMessage, NotesClient, ProviderKind};
use crate::state::AppState;

const NOT_CONFIGURED: &str = "AI not configured: set a model and API key in Settings";

/// Never returns the API key; `has_api_key` reflects env vars only.
#[tauri::command]
pub fn load_ai_config(app: AppHandle) -> Result<AiConfigView, String> {
    let config = ai::load_for(&app)?;
    Ok(AiConfigView {
        kind: config.kind,
        base_url: config.base_url,
        model: config.model,
        has_api_key: ai::env_api_key().is_some(),
    })
}

#[tauri::command]
pub fn save_ai_config(app: AppHandle, base_url: String, model: String) -> Result<(), String> {
    let config = AiConfig {
        kind: ProviderKind::OpenAiCompat,
        base_url,
        model,
    };
    ai::save(&ai::data_dir(&app)?, &config)
}

/// Round-trip a trivial prompt to verify the settings end to end.
#[tauri::command]
pub async fn test_ai_connection(app: AppHandle, api_key: String) -> Result<String, String> {
    let config = ai::load_for(&app)?;
    let api_key = ai::resolve_api_key(&api_key);
    tauri::async_runtime::spawn_blocking(move || {
        let messages = vec![ChatMessage::new("user", "Reply with the single word OK.")];
        AiClient::new().chat(&config, &api_key, messages)
    })
    .await
    .map_err(|e| format!("task failed: {e}"))?
}

/// Evaluate a finished session and persist the result so a later click shows
/// it without re-analysing.
#[tauri::command]
pub async fn analyse_session(
    app: AppHandle,
    id: i64,
    perspective: String,
    context: String,
    api_key: String,
) -> Result<AnalysisResult, String> {
    let config = ai::load_for(&app)?;
    let api_key = ai::resolve_api_key(&api_key);
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
            "session={id} model={} perspective={perspective} context={context} transcript={} chars",
            config.model,
            transcript.chars().count(),
        ),
    );

    let stored_context = context.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let client = NotesClient::new(config, api_key);
        analyse(&client, &transcript, &perspective, &context)
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
