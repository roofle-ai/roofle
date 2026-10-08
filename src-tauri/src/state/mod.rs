mod payloads;

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::Mutex;

use live_agent::KindState;
use roofle_core::storage::SqliteStorage;

use crate::engine::{Control, SourceKind};
use crate::models::ModelManager;
use crate::session_worker::SessionRegistry;

pub use payloads::{
    question_views, ChallengeUpdate, NotesUpdate, QuestionView, TitleUpdate, TranscriptCue,
};

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
    /// One question-pipeline thread per live session.
    pub sessions: SessionRegistry,
    /// Whether the follow-up questions agent may call the LLM. Toggled from the UI.
    pub followups_enabled: AtomicBool,
    /// New-character threshold that closes a paragraph (drives follow-ups and notes). From settings.
    pub char_limit: AtomicUsize,
    /// Most follow-up questions shown at once (settings, 1 to 5).
    pub max_open_questions: AtomicUsize,
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
            sessions: SessionRegistry::default(),
            followups_enabled: AtomicBool::new(true),
            char_limit: AtomicUsize::new(live_agent::DEFAULT_CHAR_LIMIT),
            max_open_questions: AtomicUsize::new(live_agent::pipeline::PipelineConfig::default().max_open_questions),
        }
    }
}
