//! Whether any app is capturing from the default microphone.

/// Asks CoreAudio; reading this property needs no microphone permission.
#[cfg(target_os = "macos")]
pub fn is_active() -> bool {
    use std::ffi::c_void;
    use std::mem::size_of;

    #[repr(C)]
    struct PropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }

    const SYSTEM_OBJECT: u32 = 1;
    const SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
    const ELEMENT_MAIN: u32 = 0;
    const DEFAULT_INPUT_DEVICE: u32 = u32::from_be_bytes(*b"dIn ");
    const IS_RUNNING_SOMEWHERE: u32 = u32::from_be_bytes(*b"gone");

    fn read_u32(object: u32, selector: u32) -> Option<u32> {
        let address = PropertyAddress { selector, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN };
        let mut value = 0u32;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: `value` and `size` outlive the call and `size` matches `value`.
        let status = unsafe {
            AudioObjectGetPropertyData(object, &address, 0, std::ptr::null(), &mut size, (&mut value as *mut u32).cast())
        };
        (status == 0).then_some(value)
    }

    read_u32(SYSTEM_OBJECT, DEFAULT_INPUT_DEVICE)
        .and_then(|device| read_u32(device, IS_RUNNING_SOMEWHERE))
        .is_some_and(|running| running != 0)
}

/// Not detectable here, so Teams calls go unnoticed on this platform.
#[cfg(not(target_os = "macos"))]
pub fn is_active() -> bool {
    false
}
