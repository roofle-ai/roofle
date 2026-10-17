//! License gate. Behind `ROOFLE_LICENSE_GATE=on` (baked in by build.rs).
//!
//! A license key is exchanged with the license endpoint for a token signed by the server's Ed25519 key and
//! bound to this machine. The token is stored in sqlite `app_config`: editing it breaks the
//! signature and copying it to another Mac fails the device check, so sqlite is safe here.

use std::sync::OnceLock;
use std::time::Duration;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use roofle_core::storage::SqliteStorage;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const GATE: &str = env!("ROOFLE_LICENSE_GATE");
const PUBKEY_HEX: &str = env!("ROOFLE_LICENSE_PUBKEY");
/// Full URL of the license endpoint; see LICENSE_API.md for the contract.
const LICENSE_URL: &str = env!("ROOFLE_LICENSE_URL");

const TOKEN_KEY: &str = "license_token";
const LICENSE_KEY: &str = "license_key";
/// Error string the frontend matches on to show the buy/activate popup.
pub const REQUIRED: &str = "license_required";

pub fn gate_enabled() -> bool {
    GATE == "on"
}

#[derive(Deserialize)]
struct Claims {
    device: String,
    exp: i64,
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

/// Token = `hex(payload_json).hex(ed25519_signature_over_payload)`.
fn verify(token: &str, pubkey_hex: &str, device: &str, now: i64) -> bool {
    let check = || -> Option<bool> {
        let (payload, sig) = token.split_once('.')?;
        let payload = hex::decode(payload).ok()?;
        let sig = Signature::from_slice(&hex::decode(sig).ok()?).ok()?;
        let key = VerifyingKey::from_bytes(&hex::decode(pubkey_hex).ok()?.try_into().ok()?).ok()?;
        key.verify(&payload, &sig).ok()?;
        let claims: Claims = serde_json::from_slice(&payload).ok()?;
        Some(claims.device == device && claims.exp > now)
    };
    check().unwrap_or(false)
}

/// True when the gate is off, or a valid token for this device is stored.
pub fn is_licensed(storage: &SqliteStorage) -> bool {
    if !gate_enabled() {
        return true;
    }
    let Some(token) = storage.get_config(TOKEN_KEY).ok().flatten() else {
        return false;
    };
    verify(&token, PUBKEY_HEX, device_id(), chrono::Utc::now().timestamp())
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
        "device_limit" => "This license is already used on 2 devices. Contact support to move it.",
        _ => "Couldn't validate the license. Please try again.",
    }
    .into()
}

fn exchange(key: &str) -> Result<String, Failure> {
    let net = || Failure::Network("Couldn't reach the license server. Check your connection and try again.".into());
    let resp = ureq::post(LICENSE_URL)
        .timeout(Duration::from_secs(15))
        .send_json(serde_json::json!({ "license_key": key, "device_id": device_id() }));
    let body: serde_json::Value = match resp {
        Ok(r) => r.into_json().map_err(|_| net())?,
        Err(ureq::Error::Status(code, r)) if code < 500 => {
            let v: serde_json::Value = r.into_json().unwrap_or_default();
            return Err(Failure::Rejected(friendly(v["error"].as_str().unwrap_or(""))));
        }
        Err(_) => return Err(net()),
    };
    let token = body["token"].as_str().ok_or_else(net)?.to_string();
    // Never trust the wire: store only what verifies for this device.
    if !verify(&token, PUBKEY_HEX, device_id(), chrono::Utc::now().timestamp()) {
        return Err(Failure::Rejected(friendly("")));
    }
    Ok(token)
}

/// Validate `key` with Supabase and persist the signed token. Blocking.
pub fn activate(storage: &SqliteStorage, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err(friendly("invalid_key"));
    }
    let token = exchange(key).map_err(|f| match f {
        Failure::Rejected(m) | Failure::Network(m) => m,
    })?;
    storage.set_config(TOKEN_KEY, &token).map_err(|e| e.to_string())?;
    storage.set_config(LICENSE_KEY, key).map_err(|e| e.to_string())
}

/// Renew the token at startup so a revoked license stops working. Offline keeps the old token.
pub fn refresh(storage: &SqliteStorage) {
    if !gate_enabled() {
        return;
    }
    let Some(key) = storage.get_config(LICENSE_KEY).ok().flatten() else {
        return;
    };
    match exchange(&key) {
        Ok(token) => {
            let _ = storage.set_config(TOKEN_KEY, &token);
        }
        Err(Failure::Rejected(_)) => {
            let _ = storage.set_config(TOKEN_KEY, "");
        }
        Err(Failure::Network(_)) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn token(sk: &SigningKey, device: &str, exp: i64) -> String {
        let payload = format!(r#"{{"device":"{device}","exp":{exp}}}"#).into_bytes();
        format!("{}.{}", hex::encode(&payload), hex::encode(sk.sign(&payload).to_bytes()))
    }

    #[test]
    fn verify_checks_signature_device_and_expiry() {
        let sk = SigningKey::from_bytes(&[7; 32]);
        let pk = hex::encode(sk.verifying_key().to_bytes());
        let good = token(&sk, "dev1", 2000);
        assert!(verify(&good, &pk, "dev1", 1000));
        assert!(!verify(&good, &pk, "dev2", 1000), "other device");
        assert!(!verify(&good, &pk, "dev1", 3000), "expired");
        let other = SigningKey::from_bytes(&[9; 32]);
        assert!(!verify(&token(&other, "dev1", 2000), &pk, "dev1", 1000), "wrong signer");
        let (payload, sig) = good.split_once('.').unwrap();
        let forged = hex::encode(String::from_utf8(hex::decode(payload).unwrap()).unwrap().replace("2000", "9999"));
        assert!(!verify(&format!("{forged}.{sig}"), &pk, "dev1", 1000), "tampered payload");
        assert!(!verify("", &pk, "dev1", 1000));
    }
}
