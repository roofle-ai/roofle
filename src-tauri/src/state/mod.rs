mod payloads;

use std::sync::mpsc::Sender;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::Mutex;

use roofle_core::storage::SqliteStorage;

use crate::engine::{Control, SourceKind};
use crate::models::ModelManager;
use crate::session_worker::SessionRegistry;

pub use payloads::{
    question_views, ChallengeUpdate, NotesUpdate, QuestionView, TitleUpdate, TranscriptCue, WrapUp,
};

pub const DEFAULT_NOTES_WINDOW_MINS: usize = 5;

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
    /// One question-pipeline thread per live session.
    pub sessions: SessionRegistry,
    /// Whether the follow-up questions agent may call the LLM. Toggled from the UI.
    pub followups_enabled: AtomicBool,
    /// New-character threshold that closes a paragraph (drives follow-ups and notes). From settings.
    pub char_limit: AtomicUsize,
    /// Most follow-up questions shown at once (settings, 1 to 5).
    pub max_open_questions: AtomicUsize,
    /// Minutes of transcript between note-generation LLM calls (settings).
    pub notes_window_mins: AtomicUsize,
    /// Per live session: the call goal the user set, fed to the notes writer.
    pub goals: Mutex<std::collections::HashMap<i64, String>>,
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
            sessions: SessionRegistry::default(),
            followups_enabled: AtomicBool::new(true),
            char_limit: AtomicUsize::new(live_agent::DEFAULT_CHAR_LIMIT),
            max_open_questions: AtomicUsize::new(live_agent::pipeline::PipelineConfig::default().max_open_questions),
            notes_window_mins: AtomicUsize::new(DEFAULT_NOTES_WINDOW_MINS),
            goals: Mutex::new(Default::default()),
        }
    }
}
