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

use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use chrono::Utc;
use roofle_core::pipeline::{
    BothSource, MicSource, Pipeline, PipelineConfig, Source, DEFAULT_RING_CAPACITY,
};
use roofle_core::storage::{
    NewSegment, NewSession, SessionDetail, SessionRepository, SessionSummary,
};
#[cfg(target_os = "macos")]
use roofle_core::pipeline::SystemSource;
use tauri::{AppHandle, Emitter, Manager};

use crate::models::ModelInfo;
use crate::state::{AppState, TranscriptCue};

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
#[tauri::command]
pub fn start_recording(
    app: AppHandle,
    state: tauri::State<AppState>,
    source: SourceKind,
    session_id: Option<i64>,
) -> Result<(), String> {
    if !source_is_implemented(source) {
        return Err(format!("source {source:?} is not implemented yet"));
    }
    *state.last_source.lock().unwrap() = source;
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
pub fn stop_recording(app: AppHandle, state: tauri::State<AppState>) -> Result<(), String> {
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
pub fn pause_recording(state: tauri::State<AppState>) -> Result<(), String> {
    request(state, |reply| Control::Pause { reply })
}

/// Resume a paused recording, continuing the same ASR session and transcript.
#[tauri::command]
pub fn resume_recording(state: tauri::State<AppState>) -> Result<(), String> {
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

/// Plain-text transcript: one finalized utterance per line.
fn render_txt(cues: &[TranscriptCue]) -> String {
    let mut out = String::new();
    for cue in cues {
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

/// Delete a session; its segments cascade.
#[tauri::command]
pub fn delete_session(state: tauri::State<AppState>, id: i64) -> Result<(), String> {
    state.storage.delete_session(id).map_err(|e| e.to_string())
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
        );

        if let Some((id, duration)) = pending_finalize.take() {
            close_session(&app, id, duration);
            active_session = None;
        }
    }

    // App is exiting: stop capture gracefully so trailing speech is flushed.
    let _ = stop_pipeline(&mut pipeline);
    if let Some(id) = active_session.take() {
        let duration = recording_started
            .map(|s| s.elapsed().saturating_sub(paused_total).as_secs_f64())
            .unwrap_or(0.0);
        close_session(&app, id, duration);
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

    // Both sources push into the ring simultaneously (~2x real-time inflow), so
    // give the ring double the default capacity to absorb scheduling jitter.
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

    let mut pipeline = Pipeline::new(PipelineConfig {
        model_dir,
        source,
        ring_capacity: Some(ring_capacity),
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
            let start_secs = offset + start.elapsed().saturating_sub(paused_total).as_secs_f64();
            app.state::<AppState>().transcript.lock().unwrap().push(TranscriptCue {
                start_secs,
                text: segment.text.clone(),
            });
            if let Some(sid) = session_id {
                *seq += 1;
                pending.push(NewSegment {
                    session_id: sid,
                    seq: *seq,
                    start_secs,
                    text: segment.text,
                    created_at: Utc::now(),
                });
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
            text: text.to_string(),
        }
    }

    /// .txt export: one finalized utterance per line.
    #[test]
    fn txt_renders_one_line_per_cue() {
        let out = render_txt(&[cue(1.0, "hello world"), cue(4.0, "second line")]);
        assert_eq!(out, "hello world\nsecond line\n");
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
