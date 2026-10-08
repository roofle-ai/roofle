//! Commands feeding the question pipeline: goal, meeting type, feedback, end.

use live_agent::pipeline::{Feedback, MeetingType};
use roofle_core::storage::QuestionStore;
use tauri::{AppHandle, State};

use crate::session_worker::Msg;
use crate::state::AppState;

#[tauri::command]
pub fn set_goal(app: AppHandle, state: State<AppState>, session_id: i64, text: String) {
    state.sessions.send(&app, session_id, Msg::SetGoal(text));
}

#[tauri::command]
pub fn set_meeting_type(
    app: AppHandle,
    state: State<AppState>,
    session_id: i64,
    kind: MeetingType,
) {
    state.sessions.send(&app, session_id, Msg::SetMeetingType(kind));
}

/// `question_id` is the card's row id; the pipeline knows it by its own key.
#[tauri::command]
pub fn question_feedback(
    app: AppHandle,
    state: State<AppState>,
    session_id: i64,
    question_id: i64,
    kind: Feedback,
) -> Result<(), String> {
    let key = state
        .storage
        .list_questions(session_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|q| q.id == question_id)
        .and_then(|q| q.question_key)
        .ok_or("question not found")?;
    state.sessions.send(&app, session_id, Msg::Feedback { key, kind });
    Ok(())
}
