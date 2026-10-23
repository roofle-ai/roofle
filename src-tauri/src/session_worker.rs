//! One thread per live session owns that session's question pipeline
//! (`live_agent::pipeline::Session`). Chunks are processed strictly in order,
//! so a slow LLM call can never race and overwrite memory, and sessions are
//! isolated by construction. The thread persists memory, questions and stage
//! runs, and publishes the question list to the UI.

use std::collections::HashMap;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::Duration;

use chrono::Utc;
use live_agent::notes::{log_step, logging_enabled};
use live_agent::pipeline::memory::SCHEMA_VERSION;
use live_agent::pipeline::{Chunk, Feedback, Line, Memory, MeetingType, Outcome, PipelineConfig, Session, UiEvent};
use roofle_core::storage::{NewPipelineQuestion, PipelineRun, QuestionStatus, QuestionStore};
use tauri::{AppHandle, Emitter, Manager};

use crate::agents::{stage_clients, StageClients};
use crate::state::{question_views, AppState, ChallengeUpdate};
use crate::{events, notify};

/// Release-timer granularity: the worker wakes at least this often.
const TICK: Duration = Duration::from_secs(5);

pub enum Msg {
    Chunk(Chunk),
    SetGoal(String),
    SetMeetingType(MeetingType),
    Feedback { key: String, kind: Feedback },
    /// Call ended: save memory and stop the thread.
    Flush,
    Shutdown,
}

/// Routes messages to per-session threads, spawning them on first use.
#[derive(Default)]
pub struct SessionRegistry(Mutex<HashMap<i64, Sender<Msg>>>);

impl SessionRegistry {
    pub fn send(&self, app: &AppHandle, session_id: i64, msg: Msg) {
        let mut map = self.0.lock().unwrap();
        // A worker that already finished (after Flush) has a dead sender: the
        // failed send hands the message back, and a fresh worker reloads memory.
        let msg = match map.get(&session_id) {
            Some(tx) => match tx.send(msg) {
                Ok(()) => return,
                Err(e) => e.0,
            },
            None => msg,
        };
        spawn_new(&mut map, app, session_id, msg);
    }

    /// Flush a worker only if one is running; never spawns one.
    pub fn flush(&self, session_id: i64) {
        if let Some(tx) = self.0.lock().unwrap().get(&session_id) {
            let _ = tx.send(Msg::Flush);
        }
    }

    pub fn shutdown(&self, session_id: i64) {
        if let Some(tx) = self.0.lock().unwrap().remove(&session_id) {
            let _ = tx.send(Msg::Shutdown);
        }
    }
}

fn spawn_new(map: &mut HashMap<i64, Sender<Msg>>, app: &AppHandle, session_id: i64, first: Msg) {
    let (tx, rx) = channel();
    let _ = tx.send(first);
    map.insert(session_id, tx);
    let app = app.clone();
    // ponytail: unbounded channel; chunks arrive at most every few seconds, so
    // the doc's drop-oldest cap and chunk merging only matter if the LLM stalls for minutes.
    std::thread::spawn(move || run(app, session_id, rx));
}

/// Hand a finished paragraph to the session's pipeline.
pub fn submit_chunk(app: &AppHandle, session_id: i64, seq: i64, lines: Vec<Line>) {
    let state = app.state::<AppState>();
    if lines.is_empty() || !state.followups_enabled.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let chunk = Chunk {
        id: seq as u32,
        t_start_ms: lines.first().map_or(0, |l| l.ts_ms),
        t_end_ms: lines.last().map_or(0, |l| l.ts_ms),
        lines,
    };
    state.sessions.send(app, session_id, Msg::Chunk(chunk));
}

fn run(app: AppHandle, session_id: i64, rx: std::sync::mpsc::Receiver<Msg>) {
    let log = logging_enabled();
    let tag = format!("pipeline session={session_id}");
    let storage = &app.state::<AppState>().storage;
    let mut session = Session::new(load_memory(&app, session_id));
    // Questions come from the user's persona; `PipelineConfig::default()` is the old gap-type flow.
    session.cfg = PipelineConfig::persona_led();
    session.mem.persona = crate::commands::persona::profile(storage);

    loop {
        let msg = rx.recv_timeout(TICK);
        let now = Utc::now().timestamp_millis();
        session.cfg.max_open_questions = app
            .state::<AppState>()
            .max_open_questions
            .load(std::sync::atomic::Ordering::Relaxed);
        let mut events = Vec::new();
        let mut seq = 0;
        match msg {
            Ok(Msg::Chunk(chunk)) => {
                seq = chunk.id as i64;
                events = on_chunk(&app, &mut session, session_id, &chunk, now, log, &tag);
            }
            Ok(Msg::SetGoal(goal)) => {
                session.mem.goal = goal;
                session.mem.goal_inferred = false;
            }
            Ok(Msg::SetMeetingType(kind)) => session.mem.meeting_type = kind,
            Ok(Msg::Feedback { key, kind }) => {
                let _ = storage.set_question_feedback(session_id, &key, kind.as_str());
                session.on_feedback(&key, kind);
                // The UI groups by status: asked/dismissed leave "Suggestions".
                let (reason, status) = match kind {
                    Feedback::Asked => ("Asked", QuestionStatus::Answered),
                    Feedback::Dismissed => ("Dismissed", QuestionStatus::Stale),
                };
                let _ = storage.resolve_question_by_key(session_id, &key, status, reason, None);
                publish(&app, session_id, 0);
            }
            // Call ended: held questions are dropped (no late noise); keep memory.
            Ok(Msg::Flush) => {
                save_memory(&app, &session, session_id);
                return;
            }
            Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        events.extend(session.on_tick(now));
        if !events.is_empty() {
            apply(&app, &session, session_id, seq, &events);
        }
        save_memory(&app, &session, session_id);
    }
    save_memory(&app, &session, session_id);
}

fn on_chunk(
    app: &AppHandle,
    session: &mut Session,
    session_id: i64,
    chunk: &Chunk,
    now: i64,
    log: bool,
    tag: &str,
) -> Vec<UiEvent> {
    let clients = match stage_clients(app) {
        Ok(c) => c,
        Err(reason) => {
            log_step(log, tag, &format!("{reason}; skipping chunk {}", chunk.id));
            return Vec::new();
        }
    };
    match session.on_chunk(chunk, &clients.stages(), now) {
        Ok((outcome, events)) => {
            log_outcome(app, session_id, chunk.id, &clients, &outcome, log, tag);
            events
        }
        Err(e) => {
            log_step(log, tag, &format!("chunk {} skipped: {e}", chunk.id));
            Vec::new()
        }
    }
}

fn log_outcome(
    app: &AppHandle,
    session_id: i64,
    chunk_id: u32,
    clients: &StageClients,
    outcome: &Outcome,
    log: bool,
    tag: &str,
) {
    let storage = &app.state::<AppState>().storage;
    for run in &outcome.runs {
        let model = match run.stage {
            "understand" => &clients.models[0],
            "write" | "ask" => &clients.models[1],
            _ => &clients.models[2],
        };
        let (output, error) = match &run.output {
            Ok(o) => (Some(o.as_str()), None),
            Err(e) => (None, Some(e.as_str())),
        };
        // Prompts can hold confidential transcript text: stored only when AI logging is on.
        let input = if log { run.input.as_str() } else { "" };
        let _ = storage.log_pipeline_run(&PipelineRun {
            session_id,
            chunk_id: chunk_id as i64,
            stage: run.stage,
            model,
            prompt_version: run.prompt_version,
            input,
            output,
            error,
        });
    }
    log_step(
        log,
        tag,
        &format!(
            "chunk {chunk_id}: gaps={} candidate={:?} queued={:?} dropped={:?}",
            outcome.gaps, outcome.candidate, outcome.queued, outcome.dropped
        ),
    );
}

/// Persist question lifecycle changes, then publish the list and notify.
fn apply(app: &AppHandle, session: &Session, session_id: i64, seq: i64, events: &[UiEvent]) {
    let storage = &app.state::<AppState>().storage;
    let mut opened = Vec::new();
    log_step(logging_enabled(), &format!("pipeline session={session_id}"), &format!("question events: {events:?}"));
    for ev in events {
        match ev {
            UiEvent::Opened { id } => {
                let Some(q) = session.mem.question(id) else { continue };
                let _ = storage.create_pipeline_question(&NewPipelineQuestion {
                    session_id,
                    key: id,
                    text: &q.text,
                    gap_type: q.gap_type.name(),
                    source_sequence_no: q.source_chunk as i64,
                    source_ts_ms: q.source_ts_ms,
                });
                opened.push(q.text.clone());
            }
            UiEvent::Resolved { id, by_chunk } => {
                let _ = storage.resolve_question_by_key(
                    session_id,
                    id,
                    QuestionStatus::Answered,
                    "Answered later in the conversation",
                    Some(*by_chunk as i64),
                );
            }
            UiEvent::Stale { id } => {
                let _ = storage.resolve_question_by_key(
                    session_id,
                    id,
                    QuestionStatus::Stale,
                    "No longer relevant",
                    None,
                );
            }
        }
    }
    publish(app, session_id, seq);
    notify::notify_new_questions(app, &opened);
}

fn publish(app: &AppHandle, session_id: i64, seq: i64) {
    let storage = &app.state::<AppState>().storage;
    let questions = question_views(storage, session_id).unwrap_or_default();
    let _ = app.emit(events::CHALLENGES, ChallengeUpdate { session_id, seq, questions });
}

fn load_memory(app: &AppHandle, session_id: i64) -> Memory {
    app.state::<AppState>()
        .storage
        .load_memory(session_id)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_else(|| Memory::new(&session_id.to_string()))
}

/// Saved only between chunks, so memory on disk is always consistent.
fn save_memory(app: &AppHandle, session: &Session, session_id: i64) {
    if let Ok(json) = serde_json::to_string(&session.mem) {
        let storage = &app.state::<AppState>().storage;
        if let Err(e) = storage.save_memory(session_id, &json, SCHEMA_VERSION) {
            eprintln!("[roofle] save pipeline memory: {e}");
        }
    }
}
