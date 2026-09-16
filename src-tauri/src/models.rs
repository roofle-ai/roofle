// roofle-app — model manager (S13).
//
// Downloads streaming ASR models on first run (with progress events + checksum
// verification) and reports which models are available. The engine thread calls
// [`ModelManager::ensure`] before starting a pipeline, so the UI can rely on
// the model being present once recording begins.
//
// Models land under `<repo>/assets/models/<id>/` (the same layout the CLI uses;
// see `scripts/fetch-assets.sh`). The manager reuses that layout so a model
// fetched via the script is picked up by the app and vice-versa.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use bzip2::read::BzDecoder;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use roofle_core::asr;

/// Event emitted with [`ModelProgress`] while a model downloads.
pub const EVENT_MODEL_PROGRESS: &str = "model-progress";

/// One downloadable streaming ASR model.
pub struct ModelCatalogEntry {
    /// Directory name under `assets/models/` (also the release artifact name).
    pub id: &'static str,
    /// Human-readable label for the UI model selector.
    pub label: &'static str,
    /// `.tar.bz2` release URL (official k2-fsa/sherpa-onnx releases).
    pub url: &'static str,
    /// Pinned sha256 of the archive. `None` until measured; while `None` the
    /// manager records the checksum of the first good download in a `.sha256`
    /// sidecar and verifies against that on later runs.
    pub sha256: Option<&'static str>,
}

/// The catalog. S13 ships the S6-selected default; add entries here (with a
/// pinned `sha256`) to offer more models in the UI.
pub const CATALOG: &[ModelCatalogEntry] = &[ModelCatalogEntry {
    id: "sherpa-onnx-whisper-base.en",
    label: "Whisper base.en (offline, higher accuracy)",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-whisper-base.en.tar.bz2",
    sha256: None,
}];

/// Info about a model for the UI (`list_models` command).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub present: bool,
}

/// Download progress for the UI (`model-progress` event payload).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelProgress {
    pub id: String,
    pub downloaded: u64,
    pub total: u64,
}

/// Manages the on-disk model catalog. Owns no mutable state, so it can live in
/// Tauri managed state.
pub struct ModelManager {
    root: PathBuf,
    /// Read-only bundled model root inside the app resources (macOS
    /// `Contents/Resources/models`). When a model is present here it is used
    /// directly and never downloaded.
    bundled: Option<PathBuf>,
}

impl ModelManager {
    /// `models_root` is the writable directory the app stores (and downloads)
    /// models in; `bundled` is an optional read-only resource root to prefer.
    pub fn with_bundled(models_root: PathBuf, bundled: Option<PathBuf>) -> Self {
        Self {
            root: models_root,
            bundled,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Directory a model lives in, preferring the bundled resource copy when it
    /// is present on disk (read-only); otherwise the writable download root.
    pub fn model_dir(&self, id: &str) -> PathBuf {
        if let Some(b) = &self.bundled {
            let bundled = b.join(id);
            if asr::model_present(&bundled) {
                return bundled;
            }
        }
        self.root.join(id)
    }

    /// All catalog entries with download status.
    pub fn list(&self) -> Vec<ModelInfo> {
        CATALOG
            .iter()
            .map(|m| ModelInfo {
                id: m.id.to_string(),
                label: m.label.to_string(),
                present: self.present(m.id),
            })
            .collect()
    }

    /// Whether all files of a model are present on disk (Whisper or streaming).
    pub fn present(&self, id: &str) -> bool {
        let dir = self.model_dir(id);
        asr::model_present(&dir)
    }

    /// Ensure a model is downloaded, verified and unpacked. Emits
    /// [`EVENT_MODEL_PROGRESS`] events via `app` while downloading. Returns the
    /// model directory.
    pub fn ensure(&self, app: &AppHandle, id: &str) -> Result<PathBuf, String> {
        let entry = CATALOG
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| format!("unknown model: {id}"))?;
        let dir = self.model_dir(id);
        if self.present(id) {
            return Ok(dir);
        }
        std::fs::create_dir_all(&self.root).map_err(|e| format!("create models dir: {e}"))?;

        let archive = self.download(app, entry)?;
        self.verify(entry, &archive)?;
        self.extract(&archive, &dir)?;

        if !self.present(id) {
            return Err("model extracted but files are incomplete".to_string());
        }
        Ok(dir)
    }

    /// Ensure the shared Silero VAD model (`silero_vad.onnx`) is present,
    /// downloading it to the models root if missing. Returns its path.
    ///
    /// The offline Whisper engine uses VAD to split the stream into clean
    /// speech utterances before decoding (prevents Whisper hallucination on
    /// raw windows), so this is required for the Whisper default model.
    pub fn ensure_vad(&self, app: &AppHandle) -> Result<PathBuf, String> {
        // Prefer the bundled VAD shipped inside the app resources.
        if let Some(b) = &self.bundled {
            let bundled = b.join("silero_vad.onnx");
            if bundled.is_file() {
                return Ok(bundled);
            }
        }
        let path = self.root.join("silero_vad.onnx");
        if path.is_file() {
            return Ok(path);
        }
        std::fs::create_dir_all(&self.root).map_err(|e| format!("create models dir: {e}"))?;

        let url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx";
        let resp = ureq::get(url)
            .call()
            .map_err(|e| format!("VAD download failed: {e}"))?;
        let total = resp
            .header("Content-Length")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        let mut reader = resp.into_reader();
        let mut file = std::fs::File::create(&path).map_err(|e| format!("VAD file: {e}"))?;
        let mut buf = [0u8; 64 * 1024];
        let mut downloaded: u64 = 0;
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("VAD read: {e}"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| format!("VAD write: {e}"))?;
            downloaded += n as u64;
            if total > 0 {
                let _ = app.emit(
                    EVENT_MODEL_PROGRESS,
                    ModelProgress {
                        id: "silero_vad.onnx".to_string(),
                        downloaded,
                        total,
                    },
                );
            }
        }
        Ok(path)
    }

    /// Download the model archive to a temp file, hashing + emitting progress.
    fn download(&self, app: &AppHandle, entry: &ModelCatalogEntry) -> Result<PathBuf, String> {
        let tmp = std::env::temp_dir().join(format!("Roofle-{}.tar.bz2", entry.id));
        let resp = ureq::get(entry.url)
            .call()
            .map_err(|e| format!("download failed: {e}"))?;
        let total = resp
            .header("Content-Length")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        let mut reader = resp.into_reader();
        let mut file = std::fs::File::create(&tmp).map_err(|e| format!("temp file: {e}"))?;
        let mut buf = [0u8; 64 * 1024];
        let mut downloaded: u64 = 0;
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("read: {e}"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| format!("write: {e}"))?;
            downloaded += n as u64;
            if total > 0 {
                let _ = app.emit(
                    EVENT_MODEL_PROGRESS,
                    ModelProgress {
                        id: entry.id.to_string(),
                        downloaded,
                        total,
                    },
                );
            }
        }
        Ok(tmp)
    }

    /// Verify the archive checksum: against the pinned catalog sha256 if set,
    /// else against the recorded `.sha256` sidecar (recording it if absent).
    fn verify(&self, entry: &ModelCatalogEntry, archive: &Path) -> Result<(), String> {
        let digest = hex::encode(Sha256::digest(
            std::fs::read(archive).map_err(|e| format!("read archive: {e}"))?,
        ));

        if let Some(expected) = entry.sha256 {
            if !digest.eq_ignore_ascii_case(expected) {
                return Err(format!(
                    "checksum mismatch for {}: expected {expected}, got {digest}",
                    entry.id
                ));
            }
            return Ok(());
        }

        let sidecar = self.root.join(format!("{}.sha256", entry.id));
        if sidecar.exists() {
            let recorded = std::fs::read_to_string(&sidecar)
                .map_err(|e| format!("read checksum sidecar: {e}"))?;
            if !digest.eq_ignore_ascii_case(recorded.trim()) {
                return Err(format!(
                    "checksum mismatch for {} (recorded {} != {digest}); delete the model to re-download",
                    entry.id, recorded.trim()
                ));
            }
        } else {
            std::fs::write(&sidecar, &digest).map_err(|e| format!("write checksum sidecar: {e}"))?;
        }
        Ok(())
    }

    /// Unpack the `.tar.bz2` archive into the model directory.
    fn extract(&self, archive: &Path, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("create model dir: {e}"))?;
        let file = std::fs::File::open(archive).map_err(|e| format!("open archive: {e}"))?;
        let mut tar = tar::Archive::new(BzDecoder::new(file));
        tar.unpack(dir).map_err(|e| format!("extract: {e}"))?;
        // Most sherpa-onnx releases wrap the files in a single top-level folder
        // named after the model (e.g. `sherpa-onnx-whisper-base.en/`). Flatten
        // it so the model files land directly under `dir`, which is what
        // `present()` and the ASR engine expect.
        flatten_single_subdir(dir);
        Ok(())
    }
}

/// If `dir` contains exactly one entry and it is a directory, move that
/// directory's contents up into `dir` and remove the now-empty subdirectory.
fn flatten_single_subdir(dir: &Path) {
    let entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    if entries.len() != 1 {
        return;
    }
    let sub = match entries.first() {
        Some(p) if p.is_dir() => p.clone(),
        _ => return,
    };
    if let Ok(child_entries) = std::fs::read_dir(&sub) {
        for child in child_entries.flatten() {
            let dest = dir.join(child.file_name());
            // Prefer a cheap rename; fall back to copy+remove for cross-device.
            if std::fs::rename(child.path(), &dest).is_err() {
                if let Ok(meta) = std::fs::metadata(child.path()) {
                    if meta.is_dir() {
                        copy_dir(child.path().as_path(), &dest);
                    } else if let Ok(data) = std::fs::read(child.path()) {
                        let _ = std::fs::write(&dest, data);
                    }
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&sub);
}

/// Recursively copy a directory tree.
fn copy_dir(src: &Path, dst: &Path) {
    let _ = std::fs::create_dir_all(dst);
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let from = entry.path();
            let to = dst.join(entry.file_name());
            if from.is_dir() {
                copy_dir(&from, &to);
            } else if let Ok(data) = std::fs::read(&from) {
                let _ = std::fs::write(&to, data);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalog always has a default model and `list` reports its state.
    #[test]
    fn catalog_lists_default_model() {
        let manager = ModelManager::with_bundled(PathBuf::from("/nonexistent"), None);
        let models = manager.list();
        assert!(!models.is_empty());
        assert_eq!(models[0].id, CATALOG[0].id);
        // Fresh manager: nothing downloaded.
        assert!(!models[0].present);
    }
}
