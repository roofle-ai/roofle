use serde::Serialize;
use tauri::State;

use crate::license;
use crate::state::AppState;

#[derive(Serialize)]
pub struct LicenseStatus {
    required: bool,
    licensed: bool,
}

#[tauri::command]
pub fn license_status(state: State<'_, AppState>) -> LicenseStatus {
    LicenseStatus { required: license::gate_enabled(), licensed: license::is_licensed(&state.storage) }
}

/// Async so the network round trip does not block the UI thread.
#[tauri::command]
pub async fn activate_license(state: State<'_, AppState>, key: String) -> Result<(), String> {
    license::activate(&state.storage, &key)
}
