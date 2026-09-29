// roofle-app — Tauri commands wrapping the roofle-core transcription pipeline.
//
// ## Why a dedicated engine thread?
//
// roofle-core's `Pipeline` is deliberately NOT `Send` (cpal streams and capture
// handles are not Send on macOS). Tauri's managed state requires `Send + Sync`,
// so the Pipeline lives on its own thread here. The frontend talks to that
// thread over a request/response channel; the thread pushes segments and levels
// back to the frontend as Tauri events. The engine thread is the only owner of
// the Pipeline, so it never has to cross a thread boundary.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use live_agent::{ParagraphWatcher, DEFAULT_CHAR_LIMIT};
use roofle_core::pipeline::{
    BothSource, MicSource, Pipeline, PipelineConfig, Source, DEFAULT_RING_CAPACITY,
};
use roofle_core::storage::{
    AudioSource, NewAudioSegment, NewSegment, NewSession, ParagraphStore, QuestionStatus,
    QuestionStore, SessionDetail, SessionRepository, SessionSummary,
};
#[cfg(target_os = "macos")]
use roofle_core::pipeline::SystemSource;
use tauri::{AppHandle, Emitter, Manager};

use crate::ai::{self, AiClient, AiConfig, AiConfigView, ChatMessage, ProviderKind};
use crate::ai_client::{ai_data_dir, NotesClient};
use crate::models::ModelInfo;
use crate::state::{AppState, TranscriptCue};
use live_agent::challenge::agent_challenges;
use live_agent::notes::{agent_notes, log_step, logging_enabled, CompletionClient};
use offline_agent::analysis::{analyse, AnalysisResult};

/// Default amplitude gain applied to captured audio (no-op at 1.0).
const DEFAULT_GAIN: f32 = 1.0;

/// How often the engine thread re-checks its control channel.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Event names emitted to the frontend.
pub const EVENT_TRANSCRIPT: &str = "transcript-update";
pub const EVENT_LEVEL: &str = "level";

/// An audio source the user can transcribe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// The microphone (implemented).
    Mic,
    /// System/device audio output (macOS S11, Windows S12).
    System,
    /// Mic + system audio mixed into one stream.
    Both,
}

/// The sources actually wired up. System audio ships in S11 (macOS only);
/// Both (mic + system mixed) needs system capture, so it is macOS-only too.
fn source_is_implemented(source: SourceKind) -> bool {
    match source {
        SourceKind::Mic => true,
        SourceKind::System => cfg!(target_os = "macos"),
        SourceKind::Both => cfg!(target_os = "macos"),
    }
}

/// Messages the frontend sends to the engine thread.
pub enum Control {
    /// Start a recording on the given source; replies via `reply`. When
    /// `session_id` is set, the recording continues that stored session
    /// instead of opening a new one.
    Start {
        source: SourceKind,
        session_id: Option<i64>,
        reply: Sender<Result<(), String>>,
    },
    /// Stop the current recording; replies via `reply`.
    Stop { reply: Sender<Result<(), String>> },
    /// Pause the live recording: capture keeps running but audio stops feeding
    /// the ASR session, so transcription pauses without flushing the transcript.
    Pause { reply: Sender<Result<(), String>> },
    /// Resume a paused recording, continuing the same ASR session + transcript.
    Resume { reply: Sender<Result<(), String>> },
    /// Shut the engine thread down (app exit).
    Shutdown,
}

/// Start recording on `source`. Returns an error string on failure. On success
/// the recording state is published (tray menu + `recording-state` event).
///
/// Async so the (potentially slow) first-run model download + load on the
/// engine thread does not block the main/UI thread.
#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    source: SourceKind,
    session_id: Option<i64>,
    api_key: String,
) -> Result<(), String> {
    if !source_is_implemented(source) {
        return Err(format!("source {source:?} is not implemented yet"));
    }
    *state.last_source.lock().unwrap() = source;
    // Capture the renderer-supplied API key (from localStorage) for live notes.
    // Only overwrite when a key is actually available, so a tray start (which
    // cannot read localStorage) reuses the last key a popup recording set.
    let resolved = ai::resolve_api_key(&api_key);
    if !resolved.is_empty() {
        *state.api_key.lock().unwrap() = resolved;
    }
    let result = request(state, |reply| Control::Start {
        source,
        session_id,
        reply,
    });
    if result.is_ok() {
        crate::set_recording_state(&app, true);
    }
    result
}

/// Stop the current recording, if any. On success the recording state is
/// cleared (tray menu + `recording-state` event).
#[tauri::command]
pub async fn stop_recording(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let result = request(state, |reply| Control::Stop { reply });
    if result.is_ok() {
        crate::set_recording_state(&app, false);
    }
    result
}

/// Pause the live recording: capture continues but audio stops feeding the ASR
/// session, so transcription pauses without flushing the transcript or ending
/// the session.
#[tauri::command]
pub async fn pause_recording(state: tauri::State<'_, AppState>) -> Result<(), String> {
    request(state, |reply| Control::Pause { reply })
}

/// Resume a paused recording, continuing the same ASR session and transcript.
#[tauri::command]
pub async fn resume_recording(state: tauri::State<'_, AppState>) -> Result<(), String> {
    request(state, |reply| Control::Resume { reply })
}

/// Send a control message to the engine thread and block for its reply.
fn request(state: tauri::State<AppState>, make: impl FnOnce(Sender<Result<(), String>>) -> Control) -> Result<(), String> {
    let tx = state
        .engine
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "engine is not running".to_string())?;
    let (reply_tx, reply_rx) = mpsc::channel();
    tx.send(make(reply_tx))
        .map_err(|_| "engine shut down".to_string())?;
    reply_rx.recv().map_err(|_| "engine shut down".to_string())?
}

// --- Model manager (S13) ------------------------------------------------

/// List the model catalog with download status, for the UI selector.
#[tauri::command]
pub fn list_models(state: tauri::State<AppState>) -> Vec<ModelInfo> {
    state.models.list()
}

/// Select which model recordings should use (must be in the catalog).
#[tauri::command]
pub fn select_model(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    if !state.models.list().iter().any(|m| m.id == id) {
        return Err(format!("unknown model: {id}"));
    }
    *state.selected_model.lock().unwrap() = id;
    Ok(())
}

/// Name of the currently selected model.
#[tauri::command]
pub fn selected_model(state: tauri::State<AppState>) -> String {
    state.selected_model.lock().unwrap().clone()
}

// --- Export (S13) -------------------------------------------------------

/// Export formats supported by `export_transcript`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Txt,
    Srt,
}

/// Write the buffered transcript to `<models_root>/../exports/` and return the
/// absolute path written.
#[tauri::command]
pub fn export_transcript(
    state: tauri::State<AppState>,
    format: ExportFormat,
) -> Result<String, String> {
    let cues = state.transcript.lock().unwrap().clone();
    if cues.is_empty() {
        return Err("nothing to export yet".to_string());
    }

    let content = match format {
        ExportFormat::Txt => render_txt(&cues),
        ExportFormat::Srt => render_srt(&cues),
    };

    let exports_dir = state
        .models
        .root()
        .parent()
        .unwrap_or_else(|| Path::new("assets"))
        .join("exports");
    std::fs::create_dir_all(&exports_dir).map_err(|e| format!("create exports dir: {e}"))?;

    let stamp = chrono_like_timestamp();
    let ext = match format {
        ExportFormat::Txt => "txt",
        ExportFormat::Srt => "srt",
    };
    let path = exports_dir.join(format!("Roofle-{stamp}.{ext}"));
    std::fs::write(&path, content).map_err(|e| format!("write export: {e}"))?;
    Ok(path.display().to_string())
}

/// Plain-text transcript: one source-tagged utterance per line.
fn render_txt(cues: &[TranscriptCue]) -> String {
    let mut out = String::new();
    for cue in cues {
        let tag = match cue.source {
            AudioSource::Mic => "[Mic] ",
            AudioSource::System => "[System] ",
        };
        out.push_str(tag);
        out.push_str(cue.text.trim());
        out.push('\n');
    }
    out
}

/// SRT subtitle file. Each finalized utterance is one cue; the cue spans from
/// its own start to the next cue's start (last cue: +2 s).
fn render_srt(cues: &[TranscriptCue]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        let end = cues
            .get(i + 1)
            .map(|n| n.start_secs)
            .unwrap_or(cue.start_secs + 2.0);
        out.push_str(&format!("{}\n", i + 1));
        out.push_str(&format!(
            "{} --> {}\n",
            srt_timestamp(cue.start_secs),
            srt_timestamp(end)
        ));
        out.push_str(&format!("{}\n\n", cue.text.trim()));
    }
    out
}

/// `HH:MM:SS,mmm` from seconds.
fn srt_timestamp(secs: f64) -> String {
    let total_ms = (secs.max(0.0) * 1000.0) as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

/// Compact local timestamp for export filenames: `YYYYMMDD-HHMMSS`.
fn chrono_like_timestamp() -> String {
    // No external date crate: format a Unix timestamp as date+time in UTC.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let rem = secs % 86_400;
    format!("{days:08}-{rem:05}")
}

/// Write arbitrary text content (the full transcription or the meeting notes)
/// to the app's `exports/` folder and return the absolute path written. Used by
/// the UI's Export menu on the main panel.
#[tauri::command]
pub fn export_text(
    state: tauri::State<AppState>,
    filename: String,
    content: String,
) -> Result<String, String> {
    if content.trim().is_empty() {
        return Err("nothing to export".to_string());
    }
    let exports_dir = state
        .models
        .root()
        .parent()
        .unwrap_or_else(|| Path::new("assets"))
        .join("exports");
    std::fs::create_dir_all(&exports_dir).map_err(|e| format!("create exports dir: {e}"))?;
    let path = exports_dir.join(filename);
    std::fs::write(&path, content).map_err(|e| format!("write export: {e}"))?;
    Ok(path.display().to_string())
}

// --- Session storage -----------------------------------------------------

/// List recorded sessions, newest first, with a first-words preview.
#[tauri::command]
pub fn list_sessions(state: tauri::State<AppState>) -> Result<Vec<SessionSummary>, String> {
    state.storage.list_sessions(None).map_err(|e| e.to_string())
}

/// One session with its ordered segments and rendered paragraph transcript.
#[tauri::command]
pub fn get_session(
    state: tauri::State<AppState>,
    id: i64,
) -> Result<Option<SessionDetail>, String> {
    state.storage.get_session(id).map_err(|e| e.to_string())
}

/// Persist the editable notes document for a session (seed/transcript + user
/// edits). The notes column is independent of the segment transcript, which
/// continues to be written as-is during recording.
#[tauri::command]
pub fn save_notes(
    state: tauri::State<AppState>,
    id: i64,
    notes: String,
) -> Result<(), String> {
    state.storage.save_notes(id, &notes).map_err(|e| e.to_string())
}

/// Delete a session; its segments cascade.
#[tauri::command]
pub fn delete_session(state: tauri::State<AppState>, id: i64) -> Result<(), String> {
    state.storage.delete_session(id).map_err(|e| e.to_string())
}

/// A session's persisted follow-up questions, for the right panel on open.
#[tauri::command]
pub fn list_session_questions(
    state: tauri::State<AppState>,
    id: i64,
) -> Result<Vec<crate::state::QuestionView>, String> {
    let qs = state.storage.list_questions(id).map_err(|e| e.to_string())?;
    Ok(qs
        .iter()
        .map(|q| crate::state::QuestionView {
            id: q.id,
            text: q.text.clone(),
            status: status_label(q.status),
        })
        .collect())
}

// --- AI client (Slice 1) ------------------------------------------------

/// Load the AI config view (base URL, model). The API key itself lives in the
/// renderer's localStorage and is never returned by Rust; `has_api_key` only
/// reflects the `ROOFLE_LOCAL_KEY` env var.
#[tauri::command]
pub fn load_ai_config(app: AppHandle) -> Result<AiConfigView, String> {
    let config = ai::load_config(&ai_data_dir(&app)?);
    Ok(AiConfigView {
        kind: config.kind,
        base_url: config.base_url,
        model: config.model,
        has_api_key: ai::env_api_key().is_some(),
    })
}

/// Persist the non-secret AI config (base URL, model). The API key is stored by
/// the renderer in localStorage and supplied per call; Rust never persists it.
#[tauri::command]
pub fn save_ai_config(
    app: AppHandle,
    base_url: String,
    model: String,
) -> Result<(), String> {
    let config = AiConfig {
        kind: ProviderKind::OpenAiCompat,
        base_url,
        model,
    };
    let dir = ai_data_dir(&app)?;
    ai::save_config(&dir, &config)
}

/// Send a trivial prompt through the configured client to verify the settings
/// end to end. Returns the model's reply text. The API key is supplied by the
/// renderer (from localStorage); the `ROOFLE_LOCAL_KEY` env var overrides it.
/// Runs off the main thread so a slow provider never freezes the UI.
#[tauri::command]
pub async fn test_ai_connection(app: AppHandle, api_key: String) -> Result<String, String> {
    let config = ai::load_config(&ai_data_dir(&app)?);
    let api_key = ai::resolve_api_key(&api_key);
    tauri::async_runtime::spawn_blocking(move || {
        let client = AiClient::new();
        client.chat(
            &config,
            &api_key,
            vec![ChatMessage {
                role: "user".to_string(),
                content: "Reply with the single word OK.".to_string(),
            }],
        )
    })
    .await
    .map_err(|e| format!("task failed: {e}"))?
}

/// Evaluate a finished session from a chosen perspective + context via the
/// offline-agent, and return the self-assessment for the dashboard. Runs on a
/// worker thread so a slow provider never freezes the UI. The API key is
/// supplied by the renderer (from localStorage); `ROOFLE_LOCAL_KEY` overrides.
/// The result is persisted per session so a later click shows it without
/// re-analyzing.
#[tauri::command]
pub async fn analyse_session(
    app: AppHandle,
    id: i64,
    perspective: String,
    context: String,
    api_key: String,
) -> Result<AnalysisResult, String> {
    let config = ai::load_config(&ai_data_dir(&app)?);
    let api_key = ai::resolve_api_key(&api_key);
    if config.model.trim().is_empty() || api_key.trim().is_empty() {
        return Err("AI not configured: set a model and API key in Settings".to_string());
    }

    // Full essay-paragraph transcript of the finished session (all segments,
    // not the editable notes document).
    let transcript = app
        .state::<AppState>()
        .storage
        .get_session(id)
        .map_err(|e| e.to_string())?
        .map(|d| d.transcript)
        .unwrap_or_default();
    if transcript.trim().is_empty() {
        return Err("this session has no transcript to analyse".to_string());
    }

    let log = logging_enabled();
    // The raw LLM request/response is logged by `NotesClient::complete`; log the
    // analysis step too so an empty/malformed reply is visible in the AI log.
    log_step(
        log,
        "offline/analyse",
        &format!(
            "session={id} model={} perspective={perspective} context={context} transcript={} chars",
            config.model,
            transcript.chars().count(),
        ),
    );

    // Keep the context for persistence; `context` itself moves into the thread.
    let stored_context = context.clone();
    let result: AnalysisResult = tauri::async_runtime::spawn_blocking(move || {
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

    // Persist so the next click on Analyse shows this without re-analyzing.
    let json = serde_json::to_string(&result).map_err(|e| e.to_string())?;
    app.state::<AppState>()
        .storage
        .save_analysis(id, &stored_context, &json)
        .map_err(|e| e.to_string())?;
    Ok(result)
}

/// A persisted offline analysis as shown to the renderer.
#[derive(serde::Serialize)]
pub struct AnalysisView {
    context: String,
    result: AnalysisResult,
}

/// The session's persisted offline analysis, if one exists. The renderer calls
/// this before opening the analyse modal: when a result is present it is shown
/// directly and the model is not called again.
#[tauri::command]
pub fn get_session_analysis(app: AppHandle, id: i64) -> Result<Option<AnalysisView>, String> {
    let stored = app
        .state::<AppState>()
        .storage
        .load_analysis(id)
        .map_err(|e| e.to_string())?;
    match stored {
        Some(s) => {
            let result: AnalysisResult =
                serde_json::from_str(&s.result_json).map_err(|e| format!("stored analysis parse: {e}"))?;
            Ok(Some(AnalysisView {
                context: s.context,
                result,
            }))
        }
        None => Ok(None),
    }
}

/// Disposable dev aid: when `ROOFLE_LOG_AI` is on, append each finalized
/// transcription line to a timestamped text file in the project root, so ASR
/// quality can be inspected offline. Remove together with its caller when no
/// longer needed.
fn log_transcription(source: AudioSource, text: &str) {
    if !logging_enabled() {
        return;
    }
    // One log file per run, named by the millisecond timestamp of the first line.
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
        root.join(format!("roofle-transcript-{stamp}.log"))
    });

    let tag = match source {
        AudioSource::Mic => "Mic",
        AudioSource::System => "System",
    };
    let stamp = Utc::now().to_rfc3339();
    let line = format!("[{stamp}] [{tag}] {text}\n");
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// Run the two-agent notes update for one paragraph boundary and emit the
/// resulting notes document to the UI. Runs on a detached worker thread so a
/// slow provider never blocks the engine loop. Every step is logged to stderr
/// when `ROOFLE_LOG_AI` is set (`1`/`true`/`on`).
fn generate_notes(app: AppHandle, session_id: i64, seq: i64, new_text: String) {
    let new_chars = new_text.chars().count();
    std::thread::spawn(move || {
        let log = logging_enabled();
        // The API key was captured into AppState when the recording started
        // (from the renderer's localStorage; `ROOFLE_LOCAL_KEY` overrides it).
        let api_key = app.state::<AppState>().api_key.lock().unwrap().clone();
        // No AI configured → skip entirely. The notes panel is notes-only;
        // never surface raw transcription there.
        let client = match ai_data_dir(&app) {
            Ok(dir) => {
                let config = ai::load_config(&dir);
                if config.model.trim().is_empty() || api_key.trim().is_empty() {
                    log_step(
                        log,
                        &format!("notes #{seq} session={session_id}"),
                        &format!(
                            "AI not configured (model={:?}, key={}); skipping (new={new_chars} chars)",
                            config.model, !api_key.trim().is_empty(),
                        ),
                    );
                    return;
                }
                log_step(
                    log,
                    &format!("notes #{seq} session={session_id}"),
                    &format!("agent start (model={}, new={new_chars} chars)", config.model),
                );
                Some(NotesClient::new(config, api_key))
            }
            Err(e) => {
                log_step(log, &format!("notes #{seq} session={session_id}"), &e);
                return;
            }
        };

        // The live note produced so far for this session (empty on first run),
        // so the agent keeps merging across chunks. `rolling_summaries` is
        // untouched; `live_notes` is the notes store.
        let storage = &app.state::<AppState>().storage;
        let existing = storage
            .latest_note(session_id)
            .ok()
            .flatten()
            .map(|n| n.text)
            .unwrap_or_default();

        let result = agent_notes(
            client.as_ref().map(|c| c as &dyn CompletionClient).unwrap(),
            &new_text,
            &existing,
            log,
        );
        match result {
            Ok(text) => {
                log_step(
                    log,
                    &format!("notes #{seq} session={session_id}"),
                    &format!("final notes = {} chars", text.chars().count()),
                );
                // Append the note generation and publish the full document so
                // the UI shows the merged/validated result.
                let _ = storage.save_note(session_id, seq, &text);
                let _ = app.emit(
                    "notes-update",
                    crate::state::NotesUpdate { seq, text },
                );
            }
            Err(e) => log_step(log, &format!("notes #{seq} session={session_id}"), &e),
        }
    });
}

/// Run the three-agent follow-up-question update for one paragraph boundary
/// and emit the resulting question list to the UI. Runs on a detached worker
/// thread so a slow provider never blocks the engine loop, and independently
/// of the notes agent (both are real-time). Every step is logged to stderr
/// when `ROOFLE_LOG_AI` is set (`1`/`true`/`on`).
fn generate_challenges(app: AppHandle, session_id: i64, seq: i64, new_text: String) {
    let new_chars = new_text.chars().count();
    std::thread::spawn(move || {
        let log = logging_enabled();
        // Same shared API key as the notes agent (captured at recording start).
        let api_key = app.state::<AppState>().api_key.lock().unwrap().clone();
        let client = match ai_data_dir(&app) {
            Ok(dir) => {
                let config = ai::load_config(&dir);
                if config.model.trim().is_empty() || api_key.trim().is_empty() {
                    log_step(
                        log,
                        &format!("challenge #{seq} session={session_id}"),
                        &format!(
                            "AI not configured (model={:?}, key={}); skipping (new={new_chars} chars)",
                            config.model, !api_key.trim().is_empty(),
                        ),
                    );
                    return;
                }
                log_step(
                    log,
                    &format!("challenge #{seq} session={session_id}"),
                    &format!("agent start (model={}, new={new_chars} chars)", config.model),
                );
                Some(NotesClient::new(config, api_key))
            }
            Err(e) => {
                log_step(log, &format!("challenge #{seq} session={session_id}"), &e);
                return;
            }
        };

        // The session's currently-open questions (id, text), so the resolver
        // can reference them by id.
        let storage = &app.state::<AppState>().storage;
        let open = storage
            .list_questions(session_id)
            .map(|qs| {
                qs.iter()
                    .filter(|q| q.status == QuestionStatus::Open)
                    .map(|q| (q.id, q.text.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let result = agent_challenges(
            client.as_ref().map(|c| c as &dyn CompletionClient).unwrap(),
            &new_text,
            &open,
            log,
        );
        match result {
            Ok(res) => {
                log_step(
                    log,
                    &format!("challenge #{seq} session={session_id}"),
                    &format!(
                        "added={} answered={} stale={}",
                        res.added.len(),
                        res.answered.len(),
                        res.stale.len(),
                    ),
                );
                // Persist the JSON-derived result: add approved questions as
                // open and resolve the answered/stale ones by id.
                for text in &res.added {
                    let _ = storage.create_question(session_id, text, seq);
                }
                for (id, reason) in &res.answered {
                    let _ = storage.resolve_question(*id, QuestionStatus::Answered, reason, Some(seq));
                }
                for (id, reason) in &res.stale {
                    let _ = storage.resolve_question(*id, QuestionStatus::Stale, reason, None);
                }
                // Publish the session's full question list so the UI renders it.
                let questions = storage
                    .list_questions(session_id)
                    .map(|qs| {
                        qs.iter()
                            .map(|q| crate::state::QuestionView {
                                id: q.id,
                                text: q.text.clone(),
                                status: status_label(q.status),
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let _ = app.emit(
                    "challenge-update",
                    crate::state::ChallengeUpdate { seq, questions },
                );
                // Surface the newly added questions as a background toast
                // (skipped while the main window is visible; see notify.rs).
                if !res.added.is_empty() {
                    crate::notify::notify_new_questions(&app, &res.added);
                }
            }
            Err(e) => log_step(log, &format!("challenge #{seq} session={session_id}"), &e),
        }
    });
}

fn status_label(status: QuestionStatus) -> String {
    match status {
        QuestionStatus::Open => "open".into(),
        QuestionStatus::Answered => "answered".into(),
        QuestionStatus::Stale => "stale".into(),
    }
}

// --- Engine thread ------------------------------------------------------

/// Spawn the pipeline engine thread and return its control channel. The thread
/// owns the (non-Send) Pipeline for its whole lifetime.
pub fn spawn_engine(app: AppHandle) -> Sender<Control> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || engine_loop(app, rx));
    tx
}

/// The engine thread: owns the Pipeline, services control messages, and relays
/// segments + levels to the frontend as Tauri events.
fn engine_loop(app: AppHandle, rx: Receiver<Control>) {
    let mut pipeline: Option<Pipeline> = None;
    let mut recording_started: Option<Instant> = None;
    // Time accumulated while paused, subtracted from segment timestamps so SRT
    // offsets stay true to the spoken audio even across pause/resume.
    let mut paused_total = Duration::ZERO;
    let mut pause_started: Option<Instant> = None;
    // Open DB session for the live recording; finalized after the flush
    // segments of the stopping poll are drained.
    let mut active_session: Option<i64> = None;
    let mut pending_finalize: Option<(i64, f64)> = None;
    let mut segment_seq: i64 = 0;
    // Paragraph watcher for the live session (Slice 1: logs + persists
    // character-threshold paragraph boundaries; the AI agent consumes these
    // later). Recreated per session; flushed on stop.
    let mut watcher: Option<ParagraphWatcher> = None;
    // Timestamp offset for a continued session (prior accumulated duration).
    let mut timestamp_offset: f64 = 0.0;

    loop {
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(Control::Start {
                source,
                session_id,
                reply,
            }) => {
                if let Err(e) = start_pipeline(&app, &mut pipeline, source) {
                    let _ = reply.send(Err(e));
                    continue;
                }
                // Fresh recording: buffer cues from zero, discard the old.
                recording_started = Some(Instant::now());
                paused_total = Duration::ZERO;
                pause_started = None;
                app.state::<AppState>().transcript.lock().unwrap().clear();

                // Open a fresh session or reopen a stored one; on failure abort
                // the recording so a live meeting is never left with no row to
                // stream into.
                match prepare_session(&app, source, session_id) {
                    Ok((id, offset, last_seq)) => {
                        active_session = Some(id);
                        timestamp_offset = offset;
                        segment_seq = last_seq;
                        // Start a paragraph watcher for this session.
                        watcher = Some(ParagraphWatcher::new(DEFAULT_CHAR_LIMIT));
                    }
                    Err(e) => {
                        let _ = stop_pipeline(&mut pipeline);
                        recording_started = None;
                        let _ = reply.send(Err(e));
                        continue;
                    }
                }
                let _ = reply.send(Ok(()));
            }
            Ok(Control::Stop { reply }) => {
                let result = stop_pipeline(&mut pipeline);
                if result.is_ok() {
                    // Defer finalize until forward_events drains the flush
                    // segments, so trailing speech lands in the session first.
                    let duration = recording_started
                        .map(|s| s.elapsed().saturating_sub(paused_total).as_secs_f64())
                        .unwrap_or(0.0);
                    pending_finalize = active_session.map(|id| (id, duration));
                    recording_started = None;
                    paused_total = Duration::ZERO;
                    pause_started = None;
                }
                let _ = reply.send(result);
            }
            Ok(Control::Pause { reply }) => {
                let result = match pipeline.as_mut() {
                    Some(p) => {
                        p.set_paused(true);
                        Ok(())
                    }
                    None => Err("not recording".to_string()),
                };
                if result.is_ok() && pause_started.is_none() {
                    pause_started = Some(Instant::now());
                }
                let _ = reply.send(result);
            }
            Ok(Control::Resume { reply }) => {
                let result = match pipeline.as_mut() {
                    Some(p) => {
                        p.set_paused(false);
                        Ok(())
                    }
                    None => Err("not recording".to_string()),
                };
                if result.is_ok() {
                    if let Some(ps) = pause_started.take() {
                        paused_total += ps.elapsed();
                    }
                }
                let _ = reply.send(result);
            }
            Ok(Control::Shutdown) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        forward_events(
            &app,
            &pipeline,
            recording_started,
            paused_total,
            active_session,
            &mut segment_seq,
            timestamp_offset,
            &mut watcher,
        );

        if let Some((id, duration)) = pending_finalize.take() {
            // Trailing segments are drained; flush any final paragraph first.
            flush_watcher(&app, &mut watcher, active_session);
            close_session(&app, id, duration);
            // Offline title agent: rename the finished session from its notes.
            crate::title::rename_session(app.clone(), id);
            active_session = None;
        }
    }

    // App is exiting: stop capture gracefully so trailing speech is flushed.
    let _ = stop_pipeline(&mut pipeline);
    let exit_session = active_session.take();
    // Persist any final paragraph still buffered at exit.
    flush_watcher(&app, &mut watcher, exit_session);
    if let Some(id) = exit_session {
        let duration = recording_started
            .map(|s| s.elapsed().saturating_sub(paused_total).as_secs_f64())
            .unwrap_or(0.0);
        close_session(&app, id, duration);
        crate::title::rename_session(app.clone(), id);
    }
}

/// Build and start a pipeline for `source`, replacing any running one. Ensures
/// the selected model is downloaded first (S13) — may emit `model-progress`.
fn start_pipeline(
    app: &AppHandle,
    slot: &mut Option<Pipeline>,
    source: SourceKind,
) -> Result<(), String> {
    if slot.is_some() {
        return Err("already recording".to_string());
    }

    let state = app.state::<AppState>();
    let model_id = state.selected_model.lock().unwrap().clone();
    let model_dir = state
        .models
        .ensure(app, &model_id)
        .map_err(|e| format!("model {model_id}: {e}"))?;

    // Both mode feeds two separate source-labelled rings (mic + system), so each
    // ring sees ~1x real-time inflow. Keep a generous capacity for both-source
    // scheduling jitter regardless.
    let ring_capacity = if source == SourceKind::Both {
        DEFAULT_RING_CAPACITY * 2
    } else {
        DEFAULT_RING_CAPACITY
    };

    let source: Box<dyn Source> = match source {
        SourceKind::Mic => Box::new(MicSource::new(None, DEFAULT_GAIN)),
        #[cfg(target_os = "macos")]
        SourceKind::System => Box::new(SystemSource::new(DEFAULT_GAIN)),
        #[cfg(not(target_os = "macos"))]
        SourceKind::System => {
            return Err("system audio is not implemented on this platform".to_string())
        }
        #[cfg(target_os = "macos")]
        SourceKind::Both => Box::new(BothSource::new(DEFAULT_GAIN)),
        #[cfg(not(target_os = "macos"))]
        SourceKind::Both => {
            return Err("system audio is not implemented on this platform".to_string())
        }
    };

    // The offline Whisper default needs the Silero VAD to segment the stream
    // into clean utterances before decoding.
    let vad_model = state.models.ensure_vad(app)?;

    let mut pipeline = Pipeline::new(PipelineConfig {
        model_dir,
        source,
        ring_capacity: Some(ring_capacity),
        vad_model: Some(vad_model),
    })
    .map_err(|e| e.to_string())?;
    pipeline.start().map_err(|e| e.to_string())?;
    *slot = Some(pipeline);
    Ok(())
}

/// Stop the running pipeline (if any), flushing any trailing speech.
fn stop_pipeline(slot: &mut Option<Pipeline>) -> Result<(), String> {
    match slot.take() {
        Some(mut pipeline) => {
            pipeline.stop();
            Ok(())
        }
        None => Err("not recording".to_string()),
    }
}

/// Open a fresh session, or reopen a stored one for continuation. Returns the
/// session id, the timestamp offset (accumulated duration for a continue, 0
/// for a fresh session) and the last segment sequence.
fn prepare_session(
    app: &AppHandle,
    source: SourceKind,
    session_id: Option<i64>,
) -> Result<(i64, f64, i64), String> {
    let storage = &app.state::<AppState>().storage;
    match session_id {
        Some(id) => {
            let offset = storage
                .continue_session(id)
                .map_err(|e| format!("continue session: {e}"))?;
            let last_seq = storage
                .last_seq(id)
                .map_err(|e| format!("continue session: {e}"))?;
            Ok((id, offset, last_seq))
        }
        None => {
            let id = open_session(app, source)?;
            Ok((id, 0.0, 0))
        }
    }
}

/// Open the DB session row for a just-started recording.
fn open_session(app: &AppHandle, source: SourceKind) -> Result<i64, String> {
    let state = app.state::<AppState>();
    let model_id = state.selected_model.lock().unwrap().clone();
    let source = match source {
        SourceKind::Mic => "mic",
        SourceKind::System => "system",
        SourceKind::Both => "both",
    };
    state
        .storage
        .create_session(NewSession {
            source: source.to_string(),
            model_id,
            started_at: Utc::now(),
        })
        .map(|s| s.id)
        .map_err(|e| format!("start session: {e}"))
}

/// Close a live session: set ended_at, duration and completed status.
fn close_session(app: &AppHandle, session_id: i64, duration_secs: f64) {
    if let Err(e) = app
        .state::<AppState>()
        .storage
        .finalize_session(session_id, Utc::now(), duration_secs)
    {
        eprintln!("[roofle] finalize session {session_id}: {e}");
    }
}

/// Drain any pending segments/levels from a live pipeline into the frontend,
/// buffering finalized segments (with timestamps) for export and writing them
/// to the open DB session. `paused_total` is wall time spent paused,
/// subtracted so exported timestamps stay aligned with the spoken audio.
fn forward_events(
    app: &AppHandle,
    pipeline: &Option<Pipeline>,
    started: Option<Instant>,
    paused_total: Duration,
    session_id: Option<i64>,
    seq: &mut i64,
    offset: f64,
    watcher: &mut Option<ParagraphWatcher>,
) {
    let Some(p) = pipeline else {
        return;
    };
    let start = started.unwrap_or_else(Instant::now);

    // Batch the poll's finalized utterances into one write transaction.
    let mut pending: Vec<NewSegment> = Vec::new();
    for segment in p.events().try_iter() {
        let _ = app.emit(EVENT_TRANSCRIPT, segment.clone());
        if segment.kind == roofle_core::asr::SegmentKind::Final {
            // Debug aid: mirror each finalized utterance (with its capture
            // source) to a local text file when ROOFLE_LOG_AI=1 (never prod).
            log_transcription(segment.source, &segment.text);
            let start_secs = offset + start.elapsed().saturating_sub(paused_total).as_secs_f64();
            app.state::<AppState>().transcript.lock().unwrap().push(TranscriptCue {
                start_secs,
                source: segment.source,
                text: segment.text.clone(),
            });
            if let Some(sid) = session_id {
                *seq += 1;
                pending.push(NewSegment {
                    session_id: sid,
                    seq: *seq,
                    start_secs,
                    text: segment.text.clone(),
                    created_at: Utc::now(),
                });
                // db-persist path: fold the segment into a source-tagged
                // paragraph and, on a boundary, finalize + append it to the
                // merged transcript log (the input to the AI agents). The
                // segment carries the capture source (mic vs system/speaker).
                ingest_segment(
                    app,
                    sid,
                    segment.source,
                    &segment.text,
                    start_secs,
                    watcher,
                );
            }
        }
    }
    if !pending.is_empty() {
        if let Err(e) = app.state::<AppState>().storage.append_segments(&pending) {
            eprintln!("[roofle] persist segments: {e}");
        }
    }
    for level in p.levels().try_iter() {
        let _ = app.emit(EVENT_LEVEL, level);
    }
}

/// Fold one utterance into a source-tagged paragraph; on a boundary, finalize
/// and append it to the merged transcript log (which triggers the agents).
fn ingest_segment(
    app: &AppHandle,
    session_id: i64,
    source: AudioSource,
    text: &str,
    start_secs: f64,
    watcher: &mut Option<ParagraphWatcher>,
) {
    let storage = &app.state::<AppState>().storage;
    let start_ms = (start_secs * 1000.0) as i64;
    if let Err(e) = storage.add_segment(&NewAudioSegment {
        session_id,
        source,
        text: text.to_string(),
        start_ms,
        // The ASR stream yields no per-segment duration yet, so the span is a
        // point; `end_ms >= start_ms` still holds.
        end_ms: start_ms,
        confidence: None,
        created_at: Utc::now(),
    }) {
        eprintln!("[roofle] persist audio segment: {e}");
        return;
    }

    let Some(w) = watcher.as_mut() else {
        return;
    };
    if w.push(text) {
        finalize_paragraph(app, session_id);
        w.reset();
    }
}

/// Finalize the session's paragraph (append to the transcript log, reset the
/// row) and trigger the notes + challenge agents on the newest chunk.
fn finalize_paragraph(app: &AppHandle, session_id: i64) {
    let storage = &app.state::<AppState>().storage;
    match storage.finalize_paragraph(session_id) {
        Ok(Some((seq, text))) => {
            generate_notes(app.clone(), session_id, seq, text.clone());
            generate_challenges(app.clone(), session_id, seq, text);
        }
        Ok(None) => {}
        Err(e) => eprintln!("[roofle] finalize paragraph: {e}"),
    }
}

/// On stop, fold any remaining buffered text into a final paragraph and append
/// it to the merged log (even below the char limit), then discard the watcher.
fn flush_watcher(app: &AppHandle, watcher: &mut Option<ParagraphWatcher>, session_id: Option<i64>) {
    let Some(w) = watcher.take() else {
        return;
    };
    if w.has_pending() {
        if let Some(sid) = session_id {
            finalize_paragraph(app, sid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend sends source names lowercase (serde `rename_all`).
    #[test]
    fn source_kind_parses_lowercase() {
        let mic: SourceKind = serde_json::from_str("\"mic\"").unwrap();
        let system: SourceKind = serde_json::from_str("\"system\"").unwrap();
        let both: SourceKind = serde_json::from_str("\"both\"").unwrap();
        assert_eq!(mic, SourceKind::Mic);
        assert_eq!(system, SourceKind::System);
        assert_eq!(both, SourceKind::Both);
    }

    /// S9/S11 gate: Mic is always wired; System and Both (mic + system) depend
    /// on system-capture availability (macOS).
    #[test]
    fn source_implementation_matrix() {
        assert!(source_is_implemented(SourceKind::Mic));
        #[cfg(target_os = "macos")]
        assert!(source_is_implemented(SourceKind::System));
        #[cfg(target_os = "macos")]
        assert!(source_is_implemented(SourceKind::Both));
        #[cfg(not(target_os = "macos"))]
        assert!(!source_is_implemented(SourceKind::Both));
    }

    /// Export formats parse lowercase (serde `rename_all`).
    #[test]
    fn export_format_parses_lowercase() {
        let txt: ExportFormat = serde_json::from_str("\"txt\"").unwrap();
        let srt: ExportFormat = serde_json::from_str("\"srt\"").unwrap();
        assert_eq!(txt, ExportFormat::Txt);
        assert_eq!(srt, ExportFormat::Srt);
    }

    fn cue(secs: f64, text: &str) -> TranscriptCue {
        TranscriptCue {
            start_secs: secs,
            source: AudioSource::Mic,
            text: text.to_string(),
        }
    }

    /// .txt export: one source-tagged utterance per line.
    #[test]
    fn txt_renders_one_line_per_cue() {
        let out = render_txt(&[cue(1.0, "hello world"), cue(4.0, "second line")]);
        assert_eq!(out, "[Mic] hello world\n[Mic] second line\n");
    }

    /// .txt export: each utterance carries its capture source (mic vs system).
    #[test]
    fn txt_labels_utterances_with_their_source() {
        let mut system = cue(1.0, "speaker audio");
        system.source = AudioSource::System;
        let out = render_txt(&[cue(1.0, "user speech"), system]);
        assert_eq!(out, "[Mic] user speech\n[System] speaker audio\n");
    }

    /// .srt export: numbered cues with HH:MM:SS,mmm timestamps.
    #[test]
    fn srt_renders_timestamped_cues() {
        let out = render_srt(&[cue(0.0, "first"), cue(3.5, "second")]);
        assert!(out.contains("00:00:00,000 --> 00:00:03,500"), "got: {out}");
        assert!(out.contains("00:00:03,500 --> 00:00:05,500"), "got: {out}");
        assert!(out.starts_with("1\n"), "first cue numbered 1, got: {out}");
        assert!(out.contains("\n2\n"), "second cue numbered 2, got: {out}");
    }
}
