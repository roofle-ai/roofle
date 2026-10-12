use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use super::catalog::{ModelProgress, EVENT_MODEL_PROGRESS};

const CHUNK_SIZE: usize = 64 * 1024;
/// A stalled connection fails (and can be retried) instead of hanging forever.
const STALL_TIMEOUT: Duration = Duration::from_secs(30);

static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn request_cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

/// Stream `url` into `dest`, emitting progress events tagged with `id`.
/// Written to `dest.part` and renamed only when complete, so `dest` is never partial.
pub fn download_file(app: &AppHandle, url: &str, dest: &Path, id: &str) -> Result<(), String> {
    CANCEL.store(false, Ordering::Relaxed);
    let mut part = PathBuf::from(dest).into_os_string();
    part.push(".part");
    let part = PathBuf::from(part);
    let result = fetch(app, url, &part, id);
    match result {
        Ok(()) => std::fs::rename(&part, dest).map_err(|e| format!("finalize download: {e}")),
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

fn fetch(app: &AppHandle, url: &str, part: &Path, id: &str) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(STALL_TIMEOUT)
        .timeout_read(STALL_TIMEOUT)
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("download failed: {e}"))?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(part).map_err(|e| format!("create {}: {e}", part.display()))?;
    let mut buf = [0u8; CHUNK_SIZE];
    let mut downloaded: u64 = 0;
    loop {
        if CANCEL.load(Ordering::Relaxed) {
            return Err("download cancelled".to_string());
        }
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("download interrupted: {e}"))?;
        if n == 0 {
            return if total > 0 && downloaded != total {
                Err(format!("download incomplete ({downloaded} of {total} bytes)"))
            } else {
                Ok(())
            };
        }
        file.write_all(&buf[..n]).map_err(|e| format!("write: {e}"))?;
        downloaded += n as u64;
        if total > 0 {
            let progress = ModelProgress {
                id: id.to_string(),
                downloaded,
                total,
            };
            let _ = app.emit(EVENT_MODEL_PROGRESS, progress);
        }
    }
}
