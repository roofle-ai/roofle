use roofle_core::storage::SqliteStorage;
use std::time::Duration;

use call_detect::CallApp;
use tauri::{App, AppHandle, Manager};

use super::{tray, window};
use crate::engine::spawn_engine;
use crate::models::ModelManager;
use crate::state::AppState;

/// Set once the first launch has shown the window.
const LAUNCHED_KEY: &str = "launched_before";
/// Give macOS a moment to place the menu-bar item before asking where it is.
const TRAY_SETTLE: Duration = Duration::from_secs(2);

/// How often to look for a newly started Zoom or Teams call.
const CALL_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Whether the menu-bar icon has a frame on some screen. A hidden item (e.g. squeezed out by the notch) has none.
fn tray_visible(app: &AppHandle) -> bool {
    let tray = app.state::<AppState>().tray.lock().unwrap().clone();
    let Some(Ok(Some(rect))) = tray.map(|t| t.rect()) else { return false };
    let pos = rect.position.to_physical::<i32>(1.0);
    let size = rect.size.to_physical::<u32>(1.0);
    size.width > 0
        && app.available_monitors().unwrap_or_default().iter().any(|m| {
            let (p, s) = (m.position(), m.size());
            pos.x >= p.x && pos.x < p.x + s.width as i32 && pos.y >= p.y && pos.y < p.y + s.height as i32
        })
}

/// Invite the user to record a detected call, unless a recording is already running.
fn on_call_started(app: &AppHandle, call: CallApp) {
    if *app.state::<AppState>().recording.lock().unwrap() {
        return;
    }
    crate::notify::notify_call_started(app, call.name());
}

pub fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    let engine = spawn_engine(app.handle().clone());

    // Writable app-data dir: the working directory is read-only in a packaged
    // .app. Models are downloaded on first use; a bundled copy, if any, wins.
    let app_data_dir = app.path().app_data_dir()?;
    let bundled_models = app.path().resource_dir().ok().map(|dir| dir.join("models"));
    let models = ModelManager::with_bundled(app_data_dir.join("models"), bundled_models);

    // Fatal on failure: every recording persists here.
    let storage = SqliteStorage::open(&app_data_dir.join("roofle.db"))?;
    app.manage(AppState::new(engine, models, storage));

    let tray = tray::build(app)?;
    *app.state::<AppState>().tray.lock().unwrap() = Some(tray);

    #[cfg(target_os = "macos")]
    crate::notify::request_permission(app.handle());

    // The app launches hidden in the menu bar. Show the window on the very first launch, and whenever the
    // menu-bar icon is not on screen, so the user always has a way in.
    let handle = app.handle().clone();
    let storage = &app.state::<AppState>().storage;
    let first_launch = storage.get_config(LAUNCHED_KEY).ok().flatten().is_none();
    let _ = storage.set_config(LAUNCHED_KEY, "1");
    if first_launch {
        window::show_popup(&handle);
    } else {
        std::thread::spawn(move || {
            std::thread::sleep(TRAY_SETTLE);
            if !tray_visible(&handle) {
                window::show_popup(&handle);
            }
        });
    }

    let handle = app.handle().clone();
    call_detect::watch(CALL_POLL_INTERVAL, move |call| on_call_started(&handle, call));

    Ok(())
}
