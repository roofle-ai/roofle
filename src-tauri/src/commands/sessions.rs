use roofle_core::storage::{SessionDetail, SessionRepository, SessionSummary};
use tauri::State;

use crate::state::{question_views, AppState, QuestionView};

fn describe(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn list_sessions(state: State<AppState>) -> Result<Vec<SessionSummary>, String> {
    state.storage.list_sessions(None).map_err(describe)
}

#[tauri::command]
pub fn get_session(state: State<AppState>, id: i64) -> Result<Option<SessionDetail>, String> {
    state.storage.get_session(id).map_err(describe)
}

/// The notes document is independent of the segment transcript.
#[tauri::command]
pub fn save_notes(state: State<AppState>, id: i64, notes: String) -> Result<(), String> {
    state.storage.save_notes(id, &notes).map_err(describe)
}

#[tauri::command]
pub fn delete_session(state: State<AppState>, id: i64) -> Result<(), String> {
    state.note_kinds.lock().unwrap().remove(&id);
    state.sessions.shutdown(id);
    state.storage.delete_session(id).map_err(describe)
}

#[tauri::command]
pub fn list_session_questions(
    state: State<AppState>,
    id: i64,
) -> Result<Vec<QuestionView>, String> {
    question_views(&state.storage, id).map_err(describe)
}
