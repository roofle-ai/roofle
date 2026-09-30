use tauri::{ActivationPolicy, AppHandle, Manager, Window, WindowEvent};

/// An `Accessory` macOS app cannot take focus, so promote it while visible.
pub fn show_popup(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Closing hides the popup; Quit in the tray menu is the only way to exit.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let _ = window.hide();
        #[cfg(target_os = "macos")]
        let _ = window
            .app_handle()
            .set_activation_policy(ActivationPolicy::Accessory);
    }
}
