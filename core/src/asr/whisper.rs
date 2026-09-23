//! Offline Whisper speech recognition via sherpa-onnx's `OfflineRecognizer`.
//!
//! Whisper is an *offline* (non-streaming) model family: you feed a complete
//! audio clip — up to the ~30 s native Whisper context — and it returns the
//! transcript. There are no partial/live hypotheses. The pipeline therefore
//! feeds fixed ~28 s windows and emits one final [`Segment`] per window, so
//! transcripts appear in ~30 s blocks instead of streaming word-by-word.
//!
//! Exported model layout (`{name}-encoder*.onnx`, `{name}-decoder*.onnx`,
//! `{name}-tokens.txt`); see `scripts/fetch-assets.sh`.

use std::path::{Path, PathBuf};

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineWhisperModelConfig, Wave};

use super::{AsrError, Segment, Transcript};

/// Sample rate (Hz) Whisper consumes; matches [`super::ASR_SAMPLE_RATE`].
pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Whisper's native audio context is 30 s; keep windows safely under it.
pub const WINDOW_SAMPLES: usize = 28 * (WHISPER_SAMPLE_RATE as usize);

/// Paths to the files of an exported Whisper ONNX model.
///
/// `from_model_dir` resolves the standard file names (for the `base.en` export:
/// `base.en-encoder.int8.onnx`, `base.en-decoder.int8.onnx`,
/// `base.en-tokens.txt`). Individual files can be overridden on the struct.
#[derive(Debug, Clone)]
pub struct WhisperModelConfig {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub tokens: PathBuf,
    /// Whisper language (e.g. `"en"`). `"auto"` disables forced language.
    pub language: String,
    /// Whisper task: `"transcribe"` or `"translate"`.
    pub task: String,
    /// ONNX Runtime execution provider (e.g. `"cpu"`).
    pub provider: String,
    /// Number of inference threads.
    pub num_threads: i32,
}

impl WhisperModelConfig {
    /// Whether `dir` looks like an exported Whisper model (has a
    /// `<name>-encoder*.onnx`). Used to auto-select the engine.
    pub fn probe(dir: &Path) -> bool {
        find_file(dir, "-encoder.int8.onnx")
            .or_else(|| find_file(dir, "-encoder.onnx"))
            .is_some()
    }

    /// Resolve a config from a model directory containing the standard files.
    pub fn from_model_dir(dir: impl AsRef<Path>) -> Result<Self, AsrError> {
        let dir = dir.as_ref();
        let encoder = find_file(dir, "-encoder.int8.onnx")
            .or_else(|| find_file(dir, "-encoder.onnx"))
            .ok_or_else(|| {
                AsrError::Create(format!(
                    "no Whisper encoder found in {} (run scripts/fetch-assets.sh)",
                    dir.display()
                ))
            })?;
        let prefix = encoder
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| {
                n.strip_suffix("-encoder.int8.onnx")
                    .or_else(|| n.strip_suffix("-encoder.onnx"))
            })
            .ok_or_else(|| AsrError::Create("cannot derive Whisper model prefix".into()))?;

        let decoder = {
            let int8 = format!("{prefix}-decoder.int8.onnx");
            let fp32 = format!("{prefix}-decoder.onnx");
            if dir.join(&int8).is_file() {
                dir.join(int8)
            } else {
                dir.join(fp32)
            }
        };
        let tokens = dir.join(format!("{prefix}-tokens.txt"));

        Ok(Self {
            encoder,
            decoder,
            tokens,
            language: "en".to_string(),
            task: "transcribe".to_string(),
            provider: "cpu".to_string(),
            num_threads: 4,
        })
    }
}

/// A loaded Whisper engine wrapping sherpa-onnx's `OfflineRecognizer`.
///
/// Inference is CPU-bound; run it off the audio callback (the pipeline already
/// runs it on the dedicated ASR thread).
pub struct WhisperEngine {
    recognizer: OfflineRecognizer,
}

impl WhisperEngine {
    /// Load the model described by `config`.
    pub fn new(cfg: WhisperModelConfig) -> Result<Self, AsrError> {
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.whisper = OfflineWhisperModelConfig {
            encoder: Some(super::path_to_string(&cfg.encoder)?),
            decoder: Some(super::path_to_string(&cfg.decoder)?),
            language: Some(cfg.language.clone()),
            task: Some(cfg.task.clone()),
            tail_paddings: -1,
            enable_token_timestamps: false,
            enable_segment_timestamps: true,
        };
        config.model_config.tokens = Some(super::path_to_string(&cfg.tokens)?);
        config.model_config.num_threads = cfg.num_threads;
        config.model_config.provider = Some(cfg.provider);

        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            AsrError::Create("sherpa-onnx could not create the Whisper recognizer".into())
        })?;
        Ok(Self { recognizer })
    }

    /// Sample rate (Hz) the model expects at its input ([`WHISPER_SAMPLE_RATE`]).
    pub fn sample_rate(&self) -> u32 {
        WHISPER_SAMPLE_RATE
    }

    /// Transcribe one audio window (must be ≤ ~30 s). Returns the text, or
    /// [`AsrError::EmptyTranscript`] when no speech was decoded.
    pub fn transcribe(&self, sample_rate: u32, samples: &[f32]) -> Result<String, AsrError> {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate as i32, samples);
        self.recognizer.decode(&stream);
        let text = stream
            .get_result()
            .map(|r| r.text.trim().to_string())
            .unwrap_or_default();
        if text.is_empty() {
            return Err(AsrError::EmptyTranscript);
        }
        Ok(text)
    }

    /// Transcribe a WAV file, chunking it into ~28 s windows (the Whisper
    /// context limit). Returns a [`Transcript`] of final segments.
    pub fn transcribe_wav<P: AsRef<Path>>(&self, path: P) -> Result<Transcript, AsrError> {
        let path_str = super::path_to_string(path.as_ref())?;
        let wave = Wave::read(&path_str).ok_or_else(|| {
            AsrError::ReadWave(format!("could not read {path_str} (does the file exist?)"))
        })?;
        let rate = wave.sample_rate() as u32;

        let mut segments = Vec::new();
        let mut parts = Vec::new();
        for chunk in wave.samples().chunks(WINDOW_SAMPLES) {
            if let Ok(text) = self.transcribe(rate, chunk) {
                // Whole-file offline transcription has no live capture source.
                segments.push(Segment::final_(crate::storage::AudioSource::Mic, text.clone()));
                parts.push(text);
            }
        }
        if segments.is_empty() {
            return Err(AsrError::EmptyTranscript);
        }
        Ok(Transcript {
            text: parts.join(" "),
            segments,
        })
    }
}

/// Find a regular file in `dir` whose name ends with `suffix`.
fn find_file(dir: &Path, suffix: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(suffix) {
                    return Some(path);
                }
            }
        }
    }
    None
}
