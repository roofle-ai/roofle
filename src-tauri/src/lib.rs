// roofle-app — thin Tauri 2 shell over roofle-core.
mod ai;
mod commands;
mod models;
mod notify;
mod state;

use roofle_core::storage::SqliteStorage;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{ActivationPolicy, Emitter, Manager};

/// Shut the engine thread down cleanly when the app exits.
fn on_exit(app: &tauri::AppHandle) {
    if let Some(tx) = app.state::<state::AppState>().engine.lock().unwrap().take() {
        let _ = tx.send(commands::Control::Shutdown);
    }
}

/// A tray menu action dispatched to the engine.
enum TrayAction {
    Start,
    Stop,
}

/// Build the tray menu reflecting the current recording state. The inactive
/// action is disabled so the menu mirrors reality.
fn build_tray_menu(app: &tauri::AppHandle, recording: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let start =
        MenuItem::with_id(app, "start", "Start Recording", !recording, Some("CmdOrCtrl+Shift+S"))?;
    let stop =
        MenuItem::with_id(app, "stop", "Stop Recording", recording, Some("CmdOrCtrl+Shift+T"))?;
    let quit = MenuItem::with_id(app, "quit", "Quit Roofle", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&start, &stop, &sep, &quit])
}

/// Persist the recording state, notify listeners, and rebuild the tray menu so
/// Start/Stop stay enabled/disabled in step with the engine.
pub fn set_recording_state(app: &tauri::AppHandle, recording: bool) {
    *app.state::<state::AppState>().recording.lock().unwrap() = recording;
    let _ = app.emit("recording-state", recording);
    if let Some(tray) = app.state::<state::AppState>().tray.lock().unwrap().as_ref() {
        match build_tray_menu(app, recording) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(e) => eprintln!("[roofle] rebuild tray menu: {e}"),
        }
    }
}

/// Run a tray action on a background thread: the engine reply can block for a
/// first-run model download, which must never freeze the main (menu) thread.
fn tray_action(app: &tauri::AppHandle, action: TrayAction) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<state::AppState>();
        let result = match action {
            TrayAction::Start => {
                let source = state.last_source.lock().unwrap().clone();
                // The tray always starts a fresh session (None = new). It passes
                // no API key (a native menu can't read the renderer's
                // localStorage), so start_recording reuses the last key a popup
                // recording supplied (or a local key env var; see ai.rs).
                tauri::async_runtime::block_on(commands::start_recording(
                    app.clone(),
                    state,
                    source,
                    None,
                    String::new(),
                ))
            }
            TrayAction::Stop => {
                tauri::async_runtime::block_on(commands::stop_recording(app.clone(), state))
            }
        };
        if let Err(e) = result {
            let _ = app.emit("app-status", format!("tray: {e}"));
        }
    });
}

/// Show and focus the transcript popup. On macOS an `Accessory` app cannot take
/// focus, so bump the activation policy while the window is visible.
fn show_popup(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        // Restore the window's size/position/maximized state on startup and
        // save it again as the user moves or resizes. VISIBLE is excluded so
        // the menu-bar app keeps launching hidden.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
        .setup(|app| {
            // Menu-bar-only app: no Dock icon (LSUIElement is also set in
            // Info.plist for the packaged build).
            #[cfg(target_os = "macos")]
            app.set_activation_policy(ActivationPolicy::Accessory);

            // Spawn the engine thread (owns the non-Send Pipeline) and expose a
            // control handle + model manager as managed state.
            let tx = commands::spawn_engine(app.handle().clone());

            // Resolve the writable app-data dir. Models are stored here (not
            // relative to the working directory, which is read-only in a
            // packaged .app), and the transcript DB lives alongside.
            let app_data_dir = app.path().app_data_dir()?;
            // Prefer models bundled inside the app resources (no runtime
            // download when present); fall back to the writable app-data dir.
            let bundled_models = app
                .path()
                .resource_dir()
                .ok()
                .map(|rd| rd.join("models"));
            let models =
                models::ModelManager::with_bundled(app_data_dir.join("models"), bundled_models);

            // Open (or create) the transcript DB in the app data dir. WAL +
            // migrations run in `SqliteStorage::open`; a failure is fatal
            // because every recording now persists here.
            let storage = SqliteStorage::open(&app_data_dir.join("roofle.db"))?;

            app.manage(state::AppState::new(tx, models, storage));

            // Tray icon in the menu bar: left-click shows the popup window;
            // right-click shows the Start/Stop/Quit menu.
            // Dedicated transparent glyph (round table + seated people) is used
            // because macOS template icons render via alpha as a monochrome
            // silhouette — the filled rounded-square app icon would appear as a
            // solid block in the menu bar.
            let tray = TrayIconBuilder::with_id("main-tray")
                .icon(
                    tauri::include_image!("icons/tray.png"),
                )
                .icon_as_template(true)
                .show_menu_on_left_click(false)
                .menu(&build_tray_menu(app.handle(), false)?)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "start" => tray_action(app, TrayAction::Start),
                    "stop" => tray_action(app, TrayAction::Stop),
                    "quit" => {
                        on_exit(app);
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_popup(tray.app_handle());
                    }
                })
                .build(app)?;
            *app.state::<state::AppState>().tray.lock().unwrap() = Some(tray);

            // macOS requires explicit notification authorization; the plugin's
            // desktop backend never prompts, so request it here (idempotent).
            #[cfg(target_os = "macos")]
            notify::request_permission();

            Ok(())
        })
        .on_window_event(|window, event| {
            // Close-to-hide: closing the popup keeps the app running in the menu
            // bar; the tray menu's Quit is the only way to exit.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                #[cfg(target_os = "macos")]
                let _ = window
                    .app_handle()
                    .set_activation_policy(ActivationPolicy::Accessory);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_recording,
            commands::pause_recording,
            commands::resume_recording,
            commands::stop_recording,
            commands::list_models,
            commands::select_model,
            commands::selected_model,
            commands::export_transcript,
            commands::list_sessions,
            commands::get_session,
            commands::delete_session,
            commands::list_session_questions,
            commands::save_notes,
            commands::load_ai_config,
            commands::save_ai_config,
            commands::test_ai_connection,
            commands::analyse_session,
            commands::get_session_analysis
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
