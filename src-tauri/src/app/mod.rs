//! Tauri application shell: builder, tray, popup window, lifecycle.

mod lifecycle;
mod setup;
pub mod tray;
mod window;

use tauri_plugin_window_state::StateFlags;

use crate::commands::{ai, export, license, models, permissions, persona, pipeline, recipes, recording, sessions};

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
            persona::get_persona,
            persona::save_persona,
            persona::describe_persona,
            license::license_status,
            license::activate_license,
            permissions::permissions_status,
            permissions::request_permission,
            recording::start_recording,
            recording::pause_recording,
            recording::resume_recording,
            recording::stop_recording,
            recording::set_followups_enabled,
            recording::set_char_limit,
            recording::set_max_open_questions,
            recording::set_notes_window_mins,
            models::list_models,
            models::select_model,
            models::selected_model,
            models::models_ready,
            models::download_model,
            models::cancel_model_download,
            export::export_transcript,
            export::export_text,
            sessions::list_sessions,
            sessions::get_session,
            sessions::delete_session,
            sessions::list_session_questions,
            sessions::list_topics,
            sessions::create_topic,
            sessions::set_session_topic,
            sessions::delete_topic,
            sessions::rename_session,
            pipeline::set_goal,
            pipeline::set_meeting_type,
            pipeline::question_feedback,
            sessions::save_notes,
            ai::load_ai_config,
            ai::save_ai_config,
            ai::test_ai_connection,
            ai::analyse_session,
            ai::get_session_analysis,
            ai::recap_session,
            ai::assist_prompt,
            ai::key_figures,
            ai::usage_report,
            ai::session_usage,
            ai::usage_first_month,
            recipes::get_action_items,
            recipes::regenerate_notes,
            recipes::get_recipe,
            recipes::run_recipe
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
