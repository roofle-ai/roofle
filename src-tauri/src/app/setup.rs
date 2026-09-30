use roofle_core::storage::SqliteStorage;
use tauri::{App, Manager};

use super::tray;
use crate::engine::spawn_engine;
use crate::models::ModelManager;
use crate::state::AppState;

pub fn setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
    // Menu-bar-only app (LSUIElement covers the packaged build).
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    let engine = spawn_engine(app.handle().clone());

    // Writable app-data dir: the working directory is read-only in a packaged
    // .app. Bundled models are preferred over downloading.
    let app_data_dir = app.path().app_data_dir()?;
    let bundled_models = app.path().resource_dir().ok().map(|dir| dir.join("models"));
    let models = ModelManager::with_bundled(app_data_dir.join("models"), bundled_models);

    // Fatal on failure: every recording persists here.
    let storage = SqliteStorage::open(&app_data_dir.join("roofle.db"))?;
    app.manage(AppState::new(engine, models, storage));

    let tray = tray::build(app)?;
    *app.state::<AppState>().tray.lock().unwrap() = Some(tray);

    #[cfg(target_os = "macos")]
    crate::notify::request_permission();

    Ok(())
}
