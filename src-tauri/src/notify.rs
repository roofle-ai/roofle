//! Background follow-up-question notifications (single responsibility).
//!
//! Roofle runs silently in the menu bar. Whenever new questions are generated,
//! this module posts a native OS notification via `tauri-plugin-notification`
//! so the user gets a heads-up without the UI opening. Clicking it does not
//! open the app; the main window still opens only via the tray icon.

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

/// Title shown on every follow-up notification.
const NOTIF_TITLE: &str = "Roofle\nFollow up question";

/// Title shown when the end-of-call recap is ready.
const CALL_READY_TITLE: &str = "Roofle\nCall ended, recap ready";

/// Whether the process is running from a proper `.app` bundle on macOS.
///
/// `UNUserNotificationCenter` requires a valid app bundle; when the binary runs
/// directly from `target/debug/` (i.e. `tauri dev` / `cargo run`), accessing
/// `currentNotificationCenter` throws an `NSInternalInconsistencyException`
/// ("bundleProxyForCurrentProcess is nil") and crashes the app. Guard so the
/// native prompt only fires for packaged builds; in dev the plugin's
/// notifications are simply not authorized (harmless).
#[cfg(target_os = "macos")]
fn is_bundled_app() -> bool {
    std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().contains(".app/"))
        .unwrap_or(false)
}

/// Request macOS notification authorization once, so follow-up notifications
/// actually display. The plugin's desktop backend never prompts for this; on
/// macOS a notification is silently dropped until the user grants it.
#[cfg(target_os = "macos")]
pub fn request_permission() {
    use block2::{DynBlock, RcBlock};
    use objc2::runtime::Bool;
    use objc2_foundation::NSError;
    use objc2_user_notifications::{UNUserNotificationCenter, UNAuthorizationOptions};

    // Skip on unbundled dev runs — see `is_bundled_app`.
    if !is_bundled_app() {
        eprintln!("[roofle] skipping notification permission (not a bundled .app)");
        return;
    }

    let center = UNUserNotificationCenter::currentNotificationCenter();
    let options = UNAuthorizationOptions::Badge
        | UNAuthorizationOptions::Sound
        | UNAuthorizationOptions::Alert;

    let handler = RcBlock::new(move |_granted: Bool, _error: *mut NSError| {
        eprintln!("[roofle] notification permission requested (macOS)");
    });
    let handler: &DynBlock<dyn Fn(Bool, *mut NSError) + 'static> = &handler;

    center.requestAuthorizationWithOptions_completionHandler(options, handler);
}

/// Surface newly generated follow-up questions as a native notification.
pub fn notify_new_questions(app: &AppHandle, questions: &[String]) {
    if questions.is_empty() {
        return;
    }
    post(app, NOTIF_TITLE, &questions.join("\n"));
}

/// Tell the user their recap is ready, unless they are already looking at the
/// window (the wrap-up card in the UI is the signal then).
pub fn notify_call_ready(app: &AppHandle, summary: &str, action_items: usize) {
    if window_in_focus(app) {
        return;
    }
    let body = match action_items {
        0 => summary.to_string(),
        1 => format!("{summary}\n1 action item"),
        n => format!("{summary}\n{n} action items"),
    };
    post(app, CALL_READY_TITLE, &body);
}

/// Whether the main window is on screen and focused.
fn window_in_focus(app: &AppHandle) -> bool {
    app.get_webview_window("main").is_some_and(|w| {
        w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false)
    })
}

fn post(app: &AppHandle, title: &str, body: &str) {
    match app.notification().builder().title(title).body(body).show() {
        Ok(()) => eprintln!("[roofle] notification: {title}"),
        Err(e) => eprintln!("[roofle] notification failed: {e}"),
    }
}
