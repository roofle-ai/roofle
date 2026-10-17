use std::collections::HashMap;

/// Settings baked into the binary at compile time: process env wins over `../.env`.
/// Baked (not read at runtime) so a user cannot switch the license gate off with an env var.
const BAKED: &[&str] = &[
    "ROOFLE_LICENSE_GATE",
    "ROOFLE_LICENSE_PUBKEY",
    "ROOFLE_LICENSE_URL",
];

fn main() {
    // screencapturekit links Swift; on end-user Macs without Xcode the runtime
    // lives in the OS (macOS 12+), so make dyld look there.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }
    bake_env();
    tauri_build::build()
}

fn bake_env() {
    println!("cargo:rerun-if-changed=../.env");
    let file: HashMap<String, String> = std::fs::read_to_string("../.env")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().trim_matches('"').to_string()))
        .collect();
    let mut vals = HashMap::new();
    for &name in BAKED {
        println!("cargo:rerun-if-env-changed={name}");
        let v = std::env::var(name).ok().or_else(|| file.get(name).cloned()).unwrap_or_default();
        println!("cargo:rustc-env={name}={v}");
        vals.insert(name, v);
    }
    if vals["ROOFLE_LICENSE_GATE"] == "on" {
        for name in &BAKED[1..] {
            assert!(!vals[name].is_empty(), "ROOFLE_LICENSE_GATE=on needs {name} in .env");
        }
    }
}
