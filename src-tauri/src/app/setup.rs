use roofle_core::storage::SqliteStorage;
use std::time::Duration;

use call_detect::CallApp;
use tauri::{App, AppHandle, Manager};

use super::tray;
use crate::engine::spawn_engine;
use crate::models::ModelManager;
use crate::state::AppState;

/// How often to look for a newly started Zoom or Teams call.
const CALL_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// Invite the user to record a detected call, unless a recording is already running.
fn on_call_started(app: &AppHandle, call: CallApp) {
    if *app.state::<AppState>().recording.lock().unwrap() {
        return;
    }
    crate::notify::notify_call_started(app, call.name());
}

pub fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    // Menu-bar-only app (LSUIElement covers the packaged build).
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    let engine = spawn_engine(app.handle().clone());

    // Writable app-data dir: the working directory is read-only in a packaged
    // .app. Models are downloaded on first use; a bundled copy, if any, wins.
    let app_data_dir = app.path().app_data_dir()?;
    let bundled_models = app.path().resource_dir().ok().map(|dir| dir.join("models"));
    let models = ModelManager::with_bundled(app_data_dir.join("models"), bundled_models);

    // Fatal on failure: every recording persists here.
    let storage = SqliteStorage::open(&app_data_dir.join("roofle.db"))?;
    app.manage(AppState::new(engine, models, storage));

    let handle = app.handle().clone();
    std::thread::spawn(move || crate::license::refresh(&handle.state::<AppState>().storage));

    let tray = tray::build(app)?;
    *app.state::<AppState>().tray.lock().unwrap() = Some(tray);

    #[cfg(target_os = "macos")]
    crate::notify::request_permission();

    let handle = app.handle().clone();
    call_detect::watch(CALL_POLL_INTERVAL, move |call| on_call_started(&handle, call));

    Ok(())
}
