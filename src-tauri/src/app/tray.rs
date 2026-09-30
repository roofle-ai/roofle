use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use super::{lifecycle, window};
use crate::commands::recording::{start_recording, stop_recording};
use crate::events;
use crate::state::AppState;

enum TrayAction {
    Start,
    Stop,
}

/// The inactive action is disabled so the menu mirrors reality.
fn build_menu(app: &AppHandle, recording: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let start =
        MenuItem::with_id(app, "start", "Start Recording", !recording, Some("CmdOrCtrl+Shift+S"))?;
    let stop =
        MenuItem::with_id(app, "stop", "Stop Recording", recording, Some("CmdOrCtrl+Shift+T"))?;
    let quit = MenuItem::with_id(app, "quit", "Quit Roofle", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    Menu::with_items(app, &[&start, &stop, &separator, &quit])
}

/// Left-click shows the popup; right-click shows the Start/Stop/Quit menu.
pub fn build(app: &tauri::App) -> tauri::Result<TrayIcon> {
    // A dedicated transparent glyph: macOS template icons render via alpha, so
    // the filled app icon would show as a solid block.
    TrayIconBuilder::with_id("main-tray")
        .icon(tauri::include_image!("icons/tray.png"))
        .icon_as_template(true)
        .show_menu_on_left_click(false)
        .menu(&build_menu(app.handle(), false)?)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "start" => run_action(app, TrayAction::Start),
            "stop" => run_action(app, TrayAction::Stop),
            "quit" => lifecycle::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::show_popup(tray.app_handle());
            }
        })
        .build(app)
}

/// Record the state, notify the UI, and rebuild the tray menu to match.
pub fn set_recording_state(app: &AppHandle, recording: bool) {
    let state = app.state::<AppState>();
    *state.recording.lock().unwrap() = recording;
    let _ = app.emit(events::RECORDING_STATE, recording);
    let tray = state.tray.lock().unwrap();
    let Some(tray) = tray.as_ref() else {
        return;
    };
    match build_menu(app, recording) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(e) => eprintln!("[roofle] rebuild tray menu: {e}"),
    }
}

/// Off the main thread: the engine reply can block on a model download.
fn run_action(app: &AppHandle, action: TrayAction) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let result = match action {
            TrayAction::Start => {
                let source = *state.last_source.lock().unwrap();
                // Fresh session, no key: the menu can't read the renderer's
                // localStorage, so `start_recording` reuses the last key.
                tauri::async_runtime::block_on(start_recording(
                    app.clone(),
                    state,
                    source,
                    None,
                    String::new(),
                ))
            }
            TrayAction::Stop => tauri::async_runtime::block_on(stop_recording(app.clone(), state)),
        };
        if let Err(e) = result {
            let _ = app.emit(events::APP_STATUS, format!("tray: {e}"));
        }
    });
}
