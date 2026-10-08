use std::io::{Read, Write};
use std::path::Path;

use tauri::{AppHandle, Emitter};

use super::catalog::{ModelProgress, EVENT_MODEL_PROGRESS};

const CHUNK_SIZE: usize = 64 * 1024;

/// Stream `url` into `dest`, emitting progress events tagged with `id`.
pub fn download_file(app: &AppHandle, url: &str, dest: &Path, id: &str) -> Result<(), String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("download failed: {e}"))?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    let mut buf = [0u8; CHUNK_SIZE];
    let mut downloaded: u64 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            return Ok(());
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
