fn main() {
    // screencapturekit links Swift; on end-user Macs without Xcode the runtime
    // lives in the OS (macOS 12+), so make dyld look there.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    tauri_build::build()
}
