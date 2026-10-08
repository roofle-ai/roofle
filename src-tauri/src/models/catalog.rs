pub const EVENT_MODEL_PROGRESS: &str = "model-progress";

pub const VAD_FILE: &str = "silero_vad.onnx";
pub const VAD_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx";

pub struct ModelCatalogEntry {
    pub id: &'static str,
    pub label: &'static str,
    pub url: &'static str,
    /// While `None`, the first good download's checksum is recorded in a
    /// `.sha256` sidecar and verified on later runs.
    pub sha256: Option<&'static str>,
}

pub const CATALOG: &[ModelCatalogEntry] = &[ModelCatalogEntry {
    id: "sherpa-onnx-whisper-base.en",
    label: "Whisper base.en (offline, higher accuracy)",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-whisper-base.en.tar.bz2",
    sha256: None,
}];

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub present: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelProgress {
    pub id: String,
    pub downloaded: u64,
    pub total: u64,
}
