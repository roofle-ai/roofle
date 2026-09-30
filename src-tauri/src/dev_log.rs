use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Append-only dev log in the project root, one file per run. Callers gate on
/// `logging_enabled()`.
pub struct RunLog {
    prefix: &'static str,
    file: OnceLock<PathBuf>,
}

impl RunLog {
    pub const fn new(prefix: &'static str) -> Self {
        Self {
            prefix,
            file: OnceLock::new(),
        }
    }

    pub fn append(&self, text: &str) {
        let path = self.file.get_or_init(|| self.new_path());
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| f.write_all(text.as_bytes()));
    }

    fn new_path(&self) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or(Path::new("."));
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        root.join(format!("{}-{stamp}.log", self.prefix))
    }
}
