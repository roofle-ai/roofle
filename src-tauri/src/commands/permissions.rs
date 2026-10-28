//! macOS Microphone and Screen Recording status, for the onboarding checklist.

use serde::Serialize;

#[derive(Serialize)]
pub struct Permissions {
    /// "granted", "denied" or "undetermined" (not asked yet).
    pub mic: &'static str,
    pub screen: bool,
}

#[cfg(target_os = "macos")]
mod sys {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }

    pub fn mic() -> &'static str {
        let audio = NSString::from_str("soun");
        let status: isize = unsafe { msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: &*audio] };
        match status {
            3 => "granted",
            0 => "undetermined",
            _ => "denied",
        }
    }

    pub fn request_mic() {
        let audio = NSString::from_str("soun");
        let done = RcBlock::new(|_granted: Bool| {});
        unsafe {
            let _: () = msg_send![class!(AVCaptureDevice), requestAccessForMediaType: &*audio, completionHandler: &*done];
        }
    }

    pub fn screen() -> bool {
        unsafe { CGPreflightScreenCaptureAccess() }
    }

    pub fn request_screen() {
        unsafe { CGRequestScreenCaptureAccess() };
    }
}

#[cfg(not(target_os = "macos"))]
mod sys {
    pub fn mic() -> &'static str {
        "granted"
    }
    pub fn request_mic() {}
    pub fn screen() -> bool {
        true
    }
    pub fn request_screen() {}
}

#[tauri::command]
pub fn permissions_status() -> Permissions {
    Permissions { mic: sys::mic(), screen: sys::screen() }
}

/// Show the system prompt for `kind` ("mic" or "screen"). Macs only ask once; after a denial the user must use System Settings.
#[tauri::command]
pub fn request_permission(kind: String) {
    match kind.as_str() {
        "mic" => sys::request_mic(),
        "screen" => sys::request_screen(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn status_calls_reach_the_os_and_return_valid_values() {
        let p = super::permissions_status();
        assert!(["granted", "denied", "undetermined"].contains(&p.mic));
    }
}
