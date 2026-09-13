// roofle-app — thin Tauri 2 shell over roofle-core.
//
// S8: placeholder `ping` command proved the IPC bridge round-trips.
// S9: commands wrap the transcription pipeline (start/stop/source) on a
//     dedicated engine thread and emit `transcript-update` + `level` events to
//     the frontend. S10a/S10b render that in the web UI.
// S11: system audio source on macOS. S13: model manager + transcript export.
// S14: macOS menu-bar app — the app launches hidden with a tray icon in the
//     menu bar; Start/Stop/Quit live in the tray menu, and left-clicking the
//     icon opens the transcript popup window. Closing the popup hides it (the
//     app keeps running); only the tray menu's Quit exits the process.

mod commands;
mod models;
mod state;

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
                commands::start_recording(app.clone(), state, source)
            }
            TrayAction::Stop => commands::stop_recording(app.clone(), state),
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
        .setup(|app| {
            // Menu-bar-only app: no Dock icon (LSUIElement is also set in
            // Info.plist for the packaged build).
            #[cfg(target_os = "macos")]
            app.set_activation_policy(ActivationPolicy::Accessory);

            // Spawn the engine thread (owns the non-Send Pipeline) and expose a
            // control handle + model manager as managed state.
            let tx = commands::spawn_engine(app.handle().clone());
            let models = models::ModelManager::new(models::ModelManager::resolve_root());
            app.manage(state::AppState::new(tx, models));

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
            commands::export_transcript
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
