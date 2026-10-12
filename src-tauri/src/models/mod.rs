//! On-disk ASR model catalog: download on first run, verify, unpack.

mod archive;
mod catalog;
mod download;

use std::path::{Path, PathBuf};

use roofle_core::asr;
use tauri::AppHandle;

pub use catalog::{ModelInfo, CATALOG};

/// Owns no mutable state, so it can live in Tauri managed state.
pub struct ModelManager {
    root: PathBuf,
    /// Read-only models shipped inside the app resources; preferred over
    /// downloading when present.
    bundled: Option<PathBuf>,
}

impl ModelManager {
    pub fn with_bundled(models_root: PathBuf, bundled: Option<PathBuf>) -> Self {
        Self {
            root: models_root,
            bundled,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn model_dir(&self, id: &str) -> PathBuf {
        if let Some(bundled) = self.bundled.as_ref().map(|b| b.join(id)) {
            if asr::model_present(&bundled) {
                return bundled;
            }
        }
        self.root.join(id)
    }

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

    fn present(&self, id: &str) -> bool {
        asr::model_present(&self.model_dir(id))
    }

    /// Download, verify and unpack a model if missing; returns its directory.
    pub fn ensure(&self, app: &AppHandle, id: &str) -> Result<PathBuf, String> {
        let entry = CATALOG
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| format!("unknown model: {id}"))?;
        let dir = self.model_dir(id);
        if self.present(id) {
            return Ok(dir);
        }
        self.create_root()?;

        let tmp = std::env::temp_dir().join(format!("Roofle-{}.tar.bz2", entry.id));
        let unpacked = download::download_file(app, entry.url, &tmp, entry.id)
            .and_then(|()| archive::verify(&self.root, entry, &tmp))
            .and_then(|()| archive::extract(&tmp, &dir));
        let _ = std::fs::remove_file(&tmp);
        unpacked?;

        if self.present(id) {
            Ok(dir)
        } else {
            Err("model extracted but files are incomplete".to_string())
        }
    }

    /// Where the Silero VAD file is, if already on disk.
    fn vad_path(&self) -> Option<PathBuf> {
        let bundled = self.bundled.as_ref().map(|b| b.join(catalog::VAD_FILE));
        bundled
            .into_iter()
            .chain([self.root.join(catalog::VAD_FILE)])
            .find(|p| p.is_file())
    }

    /// True when `id` and the VAD are both on disk, so recording can start offline.
    pub fn ready(&self, id: &str) -> bool {
        self.present(id) && self.vad_path().is_some()
    }

    /// The Silero VAD model the Whisper engine needs to segment utterances.
    pub fn ensure_vad(&self, app: &AppHandle) -> Result<PathBuf, String> {
        if let Some(path) = self.vad_path() {
            return Ok(path);
        }
        let path = self.root.join(catalog::VAD_FILE);
        self.create_root()?;
        download::download_file(app, catalog::VAD_URL, &path, catalog::VAD_FILE)
            .map_err(|e| format!("VAD {e}"))?;
        Ok(path)
    }

    /// Abort an in-flight `ensure`/`ensure_vad` download.
    pub fn cancel_download(&self) {
        download::request_cancel();
    }

    fn create_root(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.root).map_err(|e| format!("create models dir: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lists_default_model() {
        let manager = ModelManager::with_bundled(PathBuf::from("/nonexistent"), None);
        let models = manager.list();
        assert!(!models.is_empty());
        assert_eq!(models[0].id, CATALOG[0].id);
        assert!(!models[0].present);
    }
}
