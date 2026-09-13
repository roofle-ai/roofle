// roofle-app — shared Tauri state.
//
// The transcription pipeline (roofle-core) is NOT Send: cpal streams and the
// capture handles are not Send on macOS, so the Pipeline cannot live in Tauri's
// managed state. Instead a dedicated engine thread owns it (see commands.rs)
// and managed state here only carries a control channel to that thread, plus
// app-level concerns the shell owns: the model manager (S13) and the buffered
// transcript used by export (S13).

use std::sync::mpsc::Sender;
use std::sync::Mutex;

use crate::commands::{Control, SourceKind};
use crate::models::ModelManager;

/// One finalized utterance with its offset (seconds) from recording start.
/// Backs the `.txt`/`.srt` export (S13).
#[derive(Debug, Clone, serde::Serialize)]
pub struct TranscriptCue {
    /// Seconds since recording start when the utterance was finalized.
    pub start_secs: f64,
    pub text: String,
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
}

impl AppState {
    pub fn new(engine: Sender<Control>, models: ModelManager) -> Self {
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
        }
    }
}
