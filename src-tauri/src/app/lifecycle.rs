use tauri::{AppHandle, Manager};

use crate::engine::Control;
use crate::state::AppState;

/// Shut the engine thread down cleanly, then exit.
pub fn quit(app: &AppHandle) {
    let engine = app.state::<AppState>().engine.lock().unwrap().take();
    if let Some(tx) = engine {
        let _ = tx.send(Control::Shutdown);
    }
    app.exit(0);
}
