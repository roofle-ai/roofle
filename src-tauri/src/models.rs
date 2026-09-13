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

use roofle_core::asr::AsrModelConfig;

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
    id: "sherpa-onnx-streaming-zipformer-en-20M-2023-02-17",
    label: "Streaming Zipformer EN 20M (default, fastest)",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17.tar.bz2",
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
}

impl ModelManager {
    /// `models_root` is the `assets/models` directory.
    pub fn new(models_root: PathBuf) -> Self {
        Self { root: models_root }
    }

    /// Resolve `assets/models` by walking up from the working directory (the
    /// repo root during `tauri dev`), creating it if missing.
    pub fn resolve_root() -> PathBuf {
        let mut dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        loop {
            let candidate = dir.join("assets").join("models");
            if candidate.is_dir() {
                return candidate;
            }
            if !dir.pop() {
                break;
            }
        }
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("assets")
            .join("models")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Directory a model is (or will be) unpacked into.
    pub fn model_dir(&self, id: &str) -> PathBuf {
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

    /// Whether all files of a model are present on disk.
    pub fn present(&self, id: &str) -> bool {
        let dir = self.model_dir(id);
        [
            AsrModelConfig::ENCODER_FILE,
            AsrModelConfig::DECODER_FILE,
            AsrModelConfig::JOINER_FILE,
            AsrModelConfig::TOKENS_FILE,
        ]
        .iter()
        .all(|f| dir.join(f).is_file())
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

    /// Download the model archive to a temp file, hashing + emitting progress.
    fn download(&self, app: &AppHandle, entry: &ModelCatalogEntry) -> Result<PathBuf, String> {
        let tmp = std::env::temp_dir().join(format!("roofle-{}.tar.bz2", entry.id));
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
        tar.unpack(dir).map_err(|e| format!("extract: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalog always has a default model and `list` reports its state.
    #[test]
    fn catalog_lists_default_model() {
        let manager = ModelManager::new(PathBuf::from("/nonexistent"));
        let models = manager.list();
        assert!(!models.is_empty());
        assert_eq!(models[0].id, CATALOG[0].id);
        // Fresh manager: nothing downloaded.
        assert!(!models[0].present);
    }
}
