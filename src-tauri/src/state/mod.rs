mod payloads;

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use live_agent::KindState;
use roofle_core::storage::SqliteStorage;

use crate::engine::{Control, SourceKind};
use crate::models::ModelManager;

pub use payloads::{ChallengeUpdate, NotesUpdate, QuestionView, TitleUpdate, TranscriptCue};

pub struct AppState {
    /// Control channel to the engine thread (None once it has shut down).
    pub engine: Mutex<Option<Sender<Control>>>,
    pub models: ModelManager,
    pub selected_model: Mutex<String>,
    /// Finalized cues of the current recording, for `.txt`/`.srt` export.
    pub transcript: Mutex<Vec<TranscriptCue>>,
    pub recording: Mutex<bool>,
    /// Source the tray menu's Start records.
    pub last_source: Mutex<SourceKind>,
    pub tray: Mutex<Option<tauri::tray::TrayIcon>>,
    pub storage: SqliteStorage,
    /// Renderer-supplied key captured at recording start for the live agents.
    /// Never persisted.
    pub api_key: Mutex<String>,
    /// Per live session: the notes classifier's verdict so far (meeting,
    /// conversation or presentation). In memory only; a restart re-derives it.
    pub note_kinds: Mutex<HashMap<i64, KindState>>,
    /// Per live session: the follow-up agents' open vague points (statements
    /// still unresolved), carried from one chunk to the next. In memory only.
    pub vague_points: Mutex<HashMap<i64, Vec<String>>>,
    /// Whether the follow-up questions agent may call the LLM. Toggled from the UI.
    pub followups_enabled: AtomicBool,
}

impl AppState {
    pub fn new(engine: Sender<Control>, models: ModelManager, storage: SqliteStorage) -> Self {
        let default_model = models
            .list()
            .first()
            .map(|m| m.id.clone())
            .unwrap_or_default();
        Self {
            engine: Mutex::new(Some(engine)),
            models,
            selected_model: Mutex::new(default_model),
            transcript: Mutex::new(Vec::new()),
            recording: Mutex::new(false),
            last_source: Mutex::new(SourceKind::Both),
            tray: Mutex::new(None),
            storage,
            api_key: Mutex::new(String::new()),
            note_kinds: Mutex::new(HashMap::new()),
            vague_points: Mutex::new(HashMap::new()),
            followups_enabled: AtomicBool::new(true),
        }
    }
}
