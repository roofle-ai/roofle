use roofle_core::storage::{SessionDetail, SessionRepository, SessionSummary, Topic};
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
    state.goals.lock().unwrap().remove(&id);
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

#[tauri::command]
pub fn list_topics(state: State<AppState>) -> Result<Vec<Topic>, String> {
    state.storage.list_topics().map_err(describe)
}

/// Creates the topic, or returns the existing one with the same name.
#[tauri::command]
pub fn create_topic(state: State<AppState>, name: String) -> Result<Topic, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Topic name is empty".into());
    }
    state.storage.create_topic(name).map_err(describe)
}

#[tauri::command]
pub fn set_session_topic(
    state: State<AppState>,
    id: i64,
    topic_id: Option<i64>,
) -> Result<(), String> {
    state.storage.set_session_topic(id, topic_id).map_err(describe)
}

#[tauri::command]
pub fn delete_topic(state: State<AppState>, id: i64) -> Result<(), String> {
    state.storage.delete_topic(id).map_err(describe)
}

#[tauri::command]
pub fn rename_session(state: State<AppState>, id: i64, title: String) -> Result<(), String> {
    state.storage.set_title(id, title.trim()).map_err(describe)
}
