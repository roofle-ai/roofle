//! Background follow-up-question notifications (single responsibility).
//!
//! Roofle runs silently in the menu bar. Whenever new questions are generated,
//! this module posts a native OS notification via `tauri-plugin-notification`
//! so the user gets a heads-up without the UI opening. Clicking it does not
//! open the app; the main window still opens only via the tray icon.

use tauri::{AppHandle, Manager};
#[cfg(not(target_os = "macos"))]
use tauri_plugin_notification::NotificationExt;

/// Title shown on every follow-up notification.
const NOTIF_TITLE: &str = "Roofle\nFollow up question";

/// Title shown when the end-of-call recap is ready.
const CALL_READY_TITLE: &str = "Roofle\nCall ended, recap ready";

/// Title shown when a Zoom or Teams call is detected.
const CALL_STARTED_TITLE: &str = "Roofle\nCall detected";

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
pub fn request_permission(app: &AppHandle) {
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
    install_delegate(app, &center);
    let options = UNAuthorizationOptions::Badge
        | UNAuthorizationOptions::Sound
        | UNAuthorizationOptions::Alert;

    let handler = RcBlock::new(move |_granted: Bool, _error: *mut NSError| {
        eprintln!("[roofle] notification permission requested (macOS)");
    });
    let handler: &DynBlock<dyn Fn(Bool, *mut NSError) + 'static> = &handler;

    center.requestAuthorizationWithOptions_completionHandler(options, handler);
}

/// Surface newly generated follow-up questions as a native notification,
/// unless the user is already looking at the window.
pub fn notify_new_questions(app: &AppHandle, questions: &[String]) {
    if questions.is_empty() || window_in_focus(app) {
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

/// Invite the user to capture a call that just started in `call_app`.
pub fn notify_call_started(app: &AppHandle, call_app: &str) {
    post(app, CALL_STARTED_TITLE, &format!("{call_app} call started. Start a conversation in Roofle to capture notes."));
}

/// Whether the main window is on screen and focused.
fn window_in_focus(app: &AppHandle) -> bool {
    app.get_webview_window("main").is_some_and(|w| {
        w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false)
    })
}

#[cfg(target_os = "macos")]
static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

#[cfg(target_os = "macos")]
mod delegate {
    use block2::DynBlock;
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{define_class, msg_send, AnyThread};
    use objc2_foundation::{NSObject, NSObjectProtocol};
    use objc2_user_notifications::{
        UNNotification, UNNotificationPresentationOptions, UNNotificationResponse,
        UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "RoofleNotificationDelegate"]
        pub struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            // Show banners even while Roofle is the active app; callers decide whether to post.
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _n: &UNNotification,
                done: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                done.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }

            // Clicking a notification brings the window up.
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                _r: &UNNotificationResponse,
                done: &DynBlock<dyn Fn()>,
            ) {
                if let Some(app) = super::APP.get() {
                    crate::app::window::show_popup(app);
                }
                done.call(());
            }
        }
    );

    pub fn install(center: &UNUserNotificationCenter) {
        let this = Delegate::alloc().set_ivars(());
        let this: Retained<Delegate> = unsafe { msg_send![super(this), init] };
        center.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        // The center holds its delegate weakly; keep it alive for the process lifetime.
        std::mem::forget(this);
    }
}

#[cfg(target_os = "macos")]
fn install_delegate(app: &AppHandle, center: &objc2_user_notifications::UNUserNotificationCenter) {
    let _ = APP.set(app.clone());
    delegate::install(center);
}

/// macOS: post through `UNUserNotificationCenter`, the same API the permission
/// prompt uses. The plugin's backend (legacy `NSUserNotification`) is not
/// delivered for notarized builds, so it is only used on other platforms.
#[cfg(target_os = "macos")]
fn post(_app: &AppHandle, title: &str, body: &str) {
    use objc2::rc::Retained;
    use objc2_foundation::{NSError, NSString};
    use objc2_user_notifications::{
        UNMutableNotificationContent, UNNotificationRequest, UNNotificationSound,
        UNUserNotificationCenter,
    };
    use block2::RcBlock;

    if !is_bundled_app() {
        return;
    }
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    // Same title = same stack in Notification Center, so a burst doesn't flood it.
    content.setThreadIdentifier(&NSString::from_str(title));
    content.setSound(Some(&UNNotificationSound::defaultSound()));

    let id = NSString::from_str(&uuid_like());
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
    let label = title.to_string();
    let handler = RcBlock::new(move |err: *mut NSError| {
        let err: Option<Retained<NSError>> = unsafe { Retained::retain(err) };
        match err {
            None => eprintln!("[roofle] notification: {label}"),
            Some(e) => eprintln!("[roofle] notification failed: {}", e.localizedDescription()),
        }
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .addNotificationRequest_withCompletionHandler(&request, Some(&handler));
}

#[cfg(target_os = "macos")]
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("roofle-{n}")
}

#[cfg(not(target_os = "macos"))]
fn post(app: &AppHandle, title: &str, body: &str) {
    match app.notification().builder().title(title).body(body).show() {
        Ok(()) => eprintln!("[roofle] notification: {title}"),
        Err(e) => eprintln!("[roofle] notification failed: {e}"),
    }
}
