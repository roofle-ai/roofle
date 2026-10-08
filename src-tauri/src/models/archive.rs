use std::path::{Path, PathBuf};

use bzip2::read::BzDecoder;
use sha2::{Digest, Sha256};

use super::catalog::ModelCatalogEntry;

/// Check the archive against the pinned checksum, else against the recorded
/// `.sha256` sidecar (recording it on first use).
pub fn verify(root: &Path, entry: &ModelCatalogEntry, archive: &Path) -> Result<(), String> {
    let bytes = std::fs::read(archive).map_err(|e| format!("read archive: {e}"))?;
    let digest = hex::encode(Sha256::digest(bytes));

    if let Some(expected) = entry.sha256 {
        return if digest.eq_ignore_ascii_case(expected) {
            Ok(())
        } else {
            Err(format!(
                "checksum mismatch for {}: expected {expected}, got {digest}",
                entry.id
            ))
        };
    }

    let sidecar = root.join(format!("{}.sha256", entry.id));
    if !sidecar.exists() {
        return std::fs::write(&sidecar, &digest).map_err(|e| format!("write checksum sidecar: {e}"));
    }
    let recorded =
        std::fs::read_to_string(&sidecar).map_err(|e| format!("read checksum sidecar: {e}"))?;
    if digest.eq_ignore_ascii_case(recorded.trim()) {
        Ok(())
    } else {
        Err(format!(
            "checksum mismatch for {} (recorded {} != {digest}); delete the model to re-download",
            entry.id,
            recorded.trim()
        ))
    }
}

/// Unpack a `.tar.bz2` archive into `dir`, dropping a single wrapping folder.
pub fn extract(archive: &Path, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create model dir: {e}"))?;
    let file = std::fs::File::open(archive).map_err(|e| format!("open archive: {e}"))?;
    tar::Archive::new(BzDecoder::new(file))
        .unpack(dir)
        .map_err(|e| format!("extract: {e}"))?;
    flatten_single_subdir(dir);
    Ok(())
}

/// Sherpa-onnx releases wrap files in one top-level folder; the ASR engine
/// expects them directly under `dir`.
fn flatten_single_subdir(dir: &Path) {
    let entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let [sub] = entries.as_slice() else {
        return;
    };
    if !sub.is_dir() {
        return;
    }
    if let Ok(children) = std::fs::read_dir(sub) {
        for child in children.flatten() {
            move_entry(&child.path(), &dir.join(child.file_name()));
        }
    }
    let _ = std::fs::remove_dir_all(sub);
}

/// Rename, falling back to copy for cross-device moves.
fn move_entry(from: &Path, to: &Path) {
    if std::fs::rename(from, to).is_ok() {
        return;
    }
    if from.is_dir() {
        copy_dir(from, to);
    } else if let Ok(data) = std::fs::read(from) {
        let _ = std::fs::write(to, data);
    }
}

fn copy_dir(src: &Path, dst: &Path) {
    let _ = std::fs::create_dir_all(dst);
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
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
