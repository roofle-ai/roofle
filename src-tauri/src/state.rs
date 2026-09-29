use std::sync::mpsc::Sender;
use std::sync::Mutex;

use roofle_core::storage::{AudioSource, SqliteStorage};

use crate::commands::{Control, SourceKind};
use crate::models::ModelManager;

/// One finalized utterance with its offset (seconds) from recording start.
/// Backs the `.txt`/`.srt` export (S13).
#[derive(Debug, Clone, serde::Serialize)]
pub struct TranscriptCue {
    /// Seconds since recording start when the utterance was finalized.
    pub start_secs: f64,
    /// Which capture source produced this utterance (`mic` or `system`).
    pub source: AudioSource,
    pub text: String,
}

/// Payload of a `notes-update` event (AI-generated meeting notes).
#[derive(Debug, Clone, serde::Serialize)]
pub struct NotesUpdate {
    /// Paragraph number this note was generated from.
    pub seq: i64,
    /// The generated note text.
    pub text: String,
}

/// One follow-up question sent to the UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct QuestionView {
    pub id: i64,
    pub text: String,
    /// `open`, `answered`, or `stale`.
    pub status: String,
}

/// Payload of a `challenge-update` event (AI-generated follow-up questions).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChallengeUpdate {
    /// Transcript sequence these questions were generated from.
    pub seq: i64,
    /// The session's full question list, with resolution status.
    pub questions: Vec<QuestionView>,
}

/// App-wide state.
pub struct AppState {
    /// Control channel to the engine thread (None once it has shut down).
    pub engine: Mutex<Option<Sender<Control>>>,
    /// Model manager for download-on-first-run (S13).
    pub models: ModelManager,
    /// Currently selected model id (from `models::CATALOG`).
    pub selected_model: Mutex<String>,
    /// Finalized segments buffered for export, in order.
    pub transcript: Mutex<Vec<TranscriptCue>>,
    /// Whether a recording is currently live (mirrored to the tray menu and
    /// the popup UI via the `recording-state` event).
    pub recording: Mutex<bool>,
    /// Last source the user started a recording on (defaults to `Both`, so the
    /// tray menu's Start captures mic + system like the popup does); the tray
    /// menu's Start records this source.
    pub last_source: Mutex<SourceKind>,
    /// Handle to the tray icon so its menu can be rebuilt on state changes.
    pub tray: Mutex<Option<tauri::tray::TrayIcon>>,
    /// SQLite store; the engine thread writes sessions/segments in real time
    /// and command threads read them for the UI.
    pub storage: SqliteStorage,
    /// The effective API key for live notes, supplied by the renderer (from
    /// its localStorage) when a recording starts; a local key env var (see
    /// `ai::KEY_ENV_VARS`) overrides it. Rust never persists this.
    pub api_key: Mutex<String>,
}

impl AppState {
    pub fn new(engine: Sender<Control>, models: ModelManager, storage: SqliteStorage) -> Self {
        let default = models
            .list()
            .first()
            .map(|m| m.id.clone())
            .unwrap_or_default();
        Self {
            engine: Mutex::new(Some(engine)),
            models,
            selected_model: Mutex::new(default),
            transcript: Mutex::new(Vec::new()),
            recording: Mutex::new(false),
            last_source: Mutex::new(SourceKind::Both),
            tray: Mutex::new(None),
            storage,
            api_key: Mutex::new(String::new()),
        }
    }
}
