//! License gate. Behind `ROOFLE_LICENSE_GATE=on` (baked in by build.rs).
//!
//! A Polar license key is activated for this machine (activation label = device id; Polar enforces the
//! activation limit) and re-validated at each launch, which also checks the subscription is still active.
//! Uses Polar's unauthenticated customer-portal endpoints: no secret ships in the app.
//! ponytail: the "checked" stamp is plain sqlite, so a determined user can forge it; sign it if piracy matters.

use std::sync::OnceLock;
use std::time::Duration;

use roofle_core::storage::SqliteStorage;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const GATE: &str = env!("ROOFLE_LICENSE_GATE");
const ORG_ID: &str = env!("ROOFLE_POLAR_ORG_ID");
const API: &str = "https://api.polar.sh/v1/customer-portal/license-keys";
/// Longest the app keeps working without reaching Polar.
const OFFLINE_GRACE_SECS: i64 = 30 * 86400;

const LICENSE_KEY: &str = "license_key";
const ACTIVATION_KEY: &str = "license_activation";
/// `"<device_id>:<unix ts of last successful validation>"`; empty when rejected.
const CHECKED_KEY: &str = "license_checked";
/// Error string the frontend matches on to show the buy/activate popup.
pub const REQUIRED: &str = "license_required";

pub fn gate_enabled() -> bool {
    GATE == "on"
}

/// Stable per-machine id: hash of the hardware UUID, so copying app data to another Mac does not carry it.
fn device_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let raw = hardware_uuid().unwrap_or_else(|| "unknown-device".into());
        hex::encode(Sha256::digest(raw.as_bytes()))
    })
}

#[cfg(target_os = "macos")]
fn hardware_uuid() -> Option<String> {
    let out = std::process::Command::new("ioreg").args(["-rd1", "-c", "IOPlatformExpertDevice"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("IOPlatformUUID"))?;
    Some(line.split('"').nth(3)?.to_string())
}

#[cfg(not(target_os = "macos"))]
fn hardware_uuid() -> Option<String> {
    None
}

/// True when `stored` is a validation stamp for this device that is still inside the offline grace.
fn fresh(stored: &str, device: &str, now: i64) -> bool {
    let Some((dev, ts)) = stored.split_once(':') else { return false };
    dev == device && ts.parse::<i64>().is_ok_and(|t| now - t < OFFLINE_GRACE_SECS)
}

/// Err(code) unless Polar says the key is granted, unexpired and (if it has one) its subscription is live.
fn check(v: &Value, now: i64) -> Result<(), &'static str> {
    if v["status"] != "granted" {
        return Err("revoked");
    }
    let expired = v["expires_at"]
        .as_str()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .is_some_and(|t| t.timestamp() <= now);
    if expired {
        return Err("expired");
    }
    match v["subscription"]["status"].as_str() {
        None | Some("active" | "trialing") => Ok(()),
        Some(_) => Err("inactive"),
    }
}

/// True when the gate is off, or this device was validated recently.
pub fn is_licensed(storage: &SqliteStorage) -> bool {
    if !gate_enabled() {
        return true;
    }
    let Some(stored) = storage.get_config(CHECKED_KEY).ok().flatten() else {
        return false;
    };
    fresh(&stored, device_id(), chrono::Utc::now().timestamp())
}

/// Backend gate for anything that must not run unlicensed.
pub fn require(storage: &SqliteStorage) -> Result<(), String> {
    if is_licensed(storage) {
        Ok(())
    } else {
        Err(REQUIRED.into())
    }
}

enum Failure {
    /// The server said no: the key is bad, revoked, or over the device limit.
    Rejected(String),
    /// Could not ask; keep whatever is stored.
    Network(String),
}

fn friendly(code: &str) -> String {
    match code {
        "invalid_key" => "That license key isn't valid. Check it and try again.",
        "revoked" => "This license is no longer active. Contact support.",
        "expired" => "This license has expired. Contact support.",
        "inactive" => "Your subscription is no longer active. Renew it to keep using Roofle.",
        "device_limit" => "This license is already active on another device. Contact support to move it.",
        _ => "Couldn't validate the license. Please try again.",
    }
    .into()
}

/// Message code for a 4xx from Polar. 403 on activate means the activation limit is reached.
fn rejection(path: &str, status: u16) -> &'static str {
    match (status, path) {
        (404 | 422, _) => "invalid_key",
        (403, "activate") => "device_limit",
        (403, _) => "revoked",
        _ => "",
    }
}

fn post(path: &str, body: Value) -> Result<Value, Failure> {
    let net = || Failure::Network("Couldn't reach the license server. Check your connection and try again.".into());
    match ureq::post(&format!("{API}/{path}")).timeout(Duration::from_secs(15)).send_json(body) {
        Ok(r) => r.into_json().map_err(|_| net()),
        Err(ureq::Error::Status(429, _)) => Err(net()),
        Err(ureq::Error::Status(c, _)) if c < 500 => Err(Failure::Rejected(friendly(rejection(path, c)))),
        Err(_) => Err(net()),
    }
}

fn validate(key: &str, activation: &str) -> Result<(), Failure> {
    let body = json!({ "key": key, "organization_id": ORG_ID, "activation_id": activation });
    let v = post("validate", body)?;
    check(&v, chrono::Utc::now().timestamp()).map_err(|c| Failure::Rejected(friendly(c)))
}

fn stamp(storage: &SqliteStorage) -> Result<(), String> {
    let v = format!("{}:{}", device_id(), chrono::Utc::now().timestamp());
    storage.set_config(CHECKED_KEY, &v).map_err(|e| e.to_string())
}

/// Activate `key` for this machine with Polar and record it. Blocking.
pub fn activate(storage: &SqliteStorage, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err(friendly("invalid_key"));
    }
    let msg = |f| match f {
        Failure::Rejected(m) | Failure::Network(m) => m,
    };
    let stored = |k| storage.get_config(k).ok().flatten();
    // Same key already activated here: validate, don't burn a second activation slot.
    let activation = match (stored(LICENSE_KEY), stored(ACTIVATION_KEY)) {
        (Some(k), Some(a)) if k == key && !a.is_empty() => a,
        _ => {
            let body = json!({ "key": key, "organization_id": ORG_ID, "label": device_id() });
            let v = post("activate", body).map_err(msg)?;
            v["id"].as_str().ok_or_else(|| friendly(""))?.to_string()
        }
    };
    validate(key, &activation).map_err(msg)?;
    storage.set_config(LICENSE_KEY, key).map_err(|e| e.to_string())?;
    storage.set_config(ACTIVATION_KEY, &activation).map_err(|e| e.to_string())?;
    stamp(storage)
}

/// Re-validate at startup so a revoked license or lapsed subscription stops working. Offline keeps the old stamp.
pub fn refresh(storage: &SqliteStorage) {
    if !gate_enabled() {
        return;
    }
    let stored = |k| storage.get_config(k).ok().flatten();
    let (Some(key), Some(activation)) = (stored(LICENSE_KEY), stored(ACTIVATION_KEY)) else {
        return;
    };
    match validate(&key, &activation) {
        Ok(()) => {
            let _ = stamp(storage);
        }
        Err(Failure::Rejected(_)) => {
            let _ = storage.set_config(CHECKED_KEY, "");
        }
        Err(Failure::Network(_)) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_requires_granted_unexpired_active_subscription() {
        let now = 1_800_000_000;
        assert!(check(&json!({"status": "granted"}), now).is_ok(), "no subscription (one-off key)");
        assert!(check(&json!({"status": "granted", "subscription": {"status": "active"}}), now).is_ok());
        assert_eq!(check(&json!({"status": "revoked"}), now), Err("revoked"));
        assert_eq!(check(&json!({"status": "granted", "subscription": {"status": "canceled"}}), now), Err("inactive"));
        assert_eq!(check(&json!({"status": "granted", "expires_at": "2020-01-01T00:00:00Z"}), now), Err("expired"));
        assert!(check(&json!({"status": "granted", "expires_at": "2099-01-01T00:00:00Z"}), now).is_ok());
    }

    #[test]
    fn rejection_maps_403_by_endpoint() {
        assert_eq!(rejection("activate", 403), "device_limit");
        assert_eq!(rejection("validate", 403), "revoked");
        assert_eq!(rejection("activate", 404), "invalid_key");
        assert_eq!(rejection("activate", 400), "");
    }

    #[test]
    fn fresh_checks_device_and_grace() {
        assert!(fresh("dev1:1000", "dev1", 1000 + OFFLINE_GRACE_SECS - 1));
        assert!(!fresh("dev1:1000", "dev1", 1000 + OFFLINE_GRACE_SECS), "stale");
        assert!(!fresh("dev1:1000", "dev2", 1000), "other device");
        assert!(!fresh("", "dev1", 1000));
    }
}
