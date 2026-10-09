//! Tauri application shell: builder, tray, popup window, lifecycle.

mod lifecycle;
mod setup;
pub mod tray;
mod window;

use tauri_plugin_window_state::StateFlags;

use crate::commands::{ai, export, models, pipeline, recording, sessions};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // VISIBLE is excluded so the menu-bar app keeps launching hidden.
    let window_state = tauri_plugin_window_state::Builder::default()
        .with_state_flags(StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED)
        .build();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(window_state)
        .setup(setup::setup)
        .on_window_event(window::on_window_event)
        .invoke_handler(tauri::generate_handler![
            recording::start_recording,
            recording::pause_recording,
            recording::resume_recording,
            recording::stop_recording,
            recording::set_followups_enabled,
            recording::set_char_limit,
            recording::set_max_open_questions,
            models::list_models,
            models::select_model,
            models::selected_model,
            export::export_transcript,
            export::export_text,
            sessions::list_sessions,
            sessions::get_session,
            sessions::delete_session,
            sessions::list_session_questions,
            pipeline::set_goal,
            pipeline::set_meeting_type,
            pipeline::question_feedback,
            sessions::save_notes,
            ai::load_ai_config,
            ai::save_ai_config,
            ai::test_ai_connection,
            ai::analyse_session,
            ai::get_session_analysis,
            ai::recap_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
