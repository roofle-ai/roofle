use tauri::{AppHandle, State};

use crate::export::{filename_stamp, write_export, ExportFormat};
use crate::state::AppState;

/// Write the buffered transcript to Downloads; returns the path.
#[tauri::command]
pub fn export_transcript(app: AppHandle, state: State<AppState>, format: ExportFormat) -> Result<String, String> {
    let cues = state.transcript.lock().unwrap().clone();
    if cues.is_empty() {
        return Err("nothing to export yet".to_string());
    }
    let filename = format!("Roofle-{}.{}", filename_stamp(), format.extension());
    write_export(&app, &state.models, &filename, &format.render(&cues))
}

/// Write arbitrary text (full transcription or notes) to Downloads.
#[tauri::command]
pub fn export_text(
    app: AppHandle,
    state: State<AppState>,
    filename: String,
    content: String,
) -> Result<String, String> {
    if content.trim().is_empty() {
        return Err("nothing to export".to_string());
    }
    write_export(&app, &state.models, &filename, &content)
}
