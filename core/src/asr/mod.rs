//! Speech-to-text engine backed by the official `sherpa-onnx` Rust bindings.

pub mod segments;
pub mod vad;
pub mod whisper;

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, OnlineStream, Wave};

pub use segments::{Segment, SegmentKind, Transcript};

/// Sample rate (Hz) every capture source is normalized to before ASR.
pub const ASR_SAMPLE_RATE: u32 = 16_000;

/// ~0.7 s of leading silence (chosen empirically); without it the streaming
/// recognizer drops the first ~0.3 s of speech.
const WARM_UP_FRAMES: usize = (ASR_SAMPLE_RATE as usize) * 7 / 10;

#[derive(Debug)]
pub enum AsrError {
    Create(String),
    ReadWave(String),
    EmptyTranscript,
    /// sherpa-onnx needs `&str` paths.
    NonUtf8Path,
}

impl fmt::Display for AsrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsrError::Create(e) => write!(f, "failed to create ASR recognizer: {e}"),
            AsrError::ReadWave(e) => write!(f, "failed to read WAV: {e}"),
            AsrError::EmptyTranscript => {
                write!(f, "ASR produced an empty transcript for the given audio")
            }
            AsrError::NonUtf8Path => write!(f, "audio path is not valid UTF-8"),
        }
    }
}

impl Error for AsrError {}

/// Paths to the files of a streaming Zipformer-Transducer model.
#[derive(Debug, Clone)]
pub struct AsrModelConfig {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
    pub provider: String,
    pub num_threads: i32,
    pub enable_endpoint: bool,
}

impl AsrModelConfig {
    pub const ENCODER_FILE: &'static str = "encoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx";
    pub const DECODER_FILE: &'static str = "decoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx";
    pub const JOINER_FILE: &'static str = "joiner-epoch-99-avg-1-chunk-16-left-128.int8.onnx";
    pub const TOKENS_FILE: &'static str = "tokens.txt";

    pub fn from_model_dir(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        Self {
            encoder: dir.join(Self::ENCODER_FILE),
            decoder: dir.join(Self::DECODER_FILE),
            joiner: dir.join(Self::JOINER_FILE),
            tokens: dir.join(Self::TOKENS_FILE),
            provider: "cpu".to_string(),
            num_threads: 2,
            enable_endpoint: true,
        }
    }

    pub fn present_files(&self) -> bool {
        self.encoder.is_file()
            && self.decoder.is_file()
            && self.joiner.is_file()
            && self.tokens.is_file()
    }
}

/// The ASR engine for a model directory, chosen by file layout: a
/// `<name>-encoder*.onnx` means Whisper, otherwise a streaming transducer.
pub enum Engine {
    Streaming(AsrEngine),
    Whisper(whisper::WhisperEngine),
}

impl Engine {
    pub fn from_model_dir(dir: impl AsRef<Path>) -> Result<Self, AsrError> {
        let dir = dir.as_ref();
        if whisper::WhisperModelConfig::probe(dir) {
            Ok(Engine::Whisper(whisper::WhisperEngine::new(
                whisper::WhisperModelConfig::from_model_dir(dir)?,
            )?))
        } else {
            Ok(Engine::Streaming(AsrEngine::new(AsrModelConfig::from_model_dir(dir))?))
        }
    }

    pub fn is_whisper(&self) -> bool {
        matches!(self, Engine::Whisper(_))
    }

    pub fn sample_rate(&self) -> u32 {
        match self {
            Engine::Streaming(e) => e.sample_rate(),
            Engine::Whisper(e) => e.sample_rate(),
        }
    }

    pub fn transcribe_wav<P: AsRef<Path>>(&self, path: P) -> Result<Transcript, AsrError> {
        match self {
            Engine::Streaming(e) => e.transcribe_wav(path),
            Engine::Whisper(e) => e.transcribe_wav(path),
        }
    }
}

/// Whether every file of the model in `dir` is present (either layout).
pub fn model_present(dir: &Path) -> bool {
    if whisper::WhisperModelConfig::probe(dir) {
        whisper::WhisperModelConfig::from_model_dir(dir)
            .map(|c| c.encoder.is_file() && c.decoder.is_file() && c.tokens.is_file())
            .unwrap_or(false)
    } else {
        AsrModelConfig::from_model_dir(dir).present_files()
    }
}

/// A loaded streaming ASR engine. Inference is CPU-bound and runs on the
/// calling thread; keep it off the real-time audio callback.
pub struct AsrEngine {
    recognizer: OnlineRecognizer,
}

impl AsrEngine {
    pub fn new(config: AsrModelConfig) -> Result<Self, AsrError> {
        let mut recognizer_config = OnlineRecognizerConfig::default();
        let model_config = &mut recognizer_config.model_config;
        model_config.transducer.encoder = Some(path_to_string(&config.encoder)?);
        model_config.transducer.decoder = Some(path_to_string(&config.decoder)?);
        model_config.transducer.joiner = Some(path_to_string(&config.joiner)?);
        model_config.tokens = Some(path_to_string(&config.tokens)?);
        model_config.provider = Some(config.provider);
        model_config.num_threads = config.num_threads;
        recognizer_config.enable_endpoint = config.enable_endpoint;
        recognizer_config.decoding_method = Some("greedy_search".to_string());

        let recognizer = OnlineRecognizer::create(&recognizer_config).ok_or_else(|| {
            AsrError::Create("sherpa-onnx could not create the recognizer".into())
        })?;
        Ok(Self { recognizer })
    }

    pub fn sample_rate(&self) -> u32 {
        ASR_SAMPLE_RATE
    }

    /// Start a live session, primed with [`WARM_UP_FRAMES`] of silence.
    pub fn stream(&self) -> StreamingSession<'_> {
        let session = StreamingSession {
            recognizer: &self.recognizer,
            stream: self.recognizer.create_stream(),
        };
        let warm_up = vec![0.0f32; WARM_UP_FRAMES];
        session.feed(&warm_up);
        session
    }

    pub fn transcribe_wav<P: AsRef<Path>>(&self, path: P) -> Result<Transcript, AsrError> {
        let path_str = path_to_string(path.as_ref())?;
        let wave = Wave::read(&path_str).ok_or_else(|| {
            AsrError::ReadWave(format!("could not read {path_str} (does the file exist?)"))
        })?;
        self.transcribe_waveform(wave.sample_rate() as u32, wave.samples())
    }

    /// Run the recognizer over a complete waveform already at [`ASR_SAMPLE_RATE`].
    pub fn transcribe_waveform(
        &self,
        sample_rate: u32,
        samples: &[f32],
    ) -> Result<Transcript, AsrError> {
        let stream = self.recognizer.create_stream();
        let rate = sample_rate as i32;

        let mut audio = Vec::with_capacity(samples.len() + WARM_UP_FRAMES);
        audio.resize(WARM_UP_FRAMES, 0.0);
        audio.extend_from_slice(samples);

        // ~0.2 s chunks mirror live streaming and are more robust at both ends
        // of the utterance than one whole-file feed.
        let chunk_frames = (sample_rate as usize) / 5;
        for chunk in audio.chunks(chunk_frames.max(1)) {
            stream.accept_waveform(rate, chunk);
            while self.recognizer.is_ready(&stream) {
                self.recognizer.decode(&stream);
            }
        }
        stream.input_finished();
        while self.recognizer.is_ready(&stream) {
            self.recognizer.decode(&stream);
        }

        let text = self
            .recognizer
            .get_result(&stream)
            .map(|r| r.text)
            .unwrap_or_default();
        if text.is_empty() {
            return Err(AsrError::EmptyTranscript);
        }
        Ok(Transcript::from_final(text))
    }
}

/// A live recognition session, created via [`AsrEngine::stream`].
///
/// Feed chunks with `feed`, read the hypothesis with `partial`, close each
/// utterance with `is_endpoint` + `take_final`, and call `finish` once at
/// shutdown. Drive it from a single consumer thread, never the audio callback.
pub struct StreamingSession<'a> {
    recognizer: &'a OnlineRecognizer,
    stream: OnlineStream,
}

impl StreamingSession<'_> {
    pub fn sample_rate(&self) -> u32 {
        ASR_SAMPLE_RATE
    }

    /// Feed a chunk of any size and decode.
    pub fn feed(&self, samples: &[f32]) {
        self.stream.accept_waveform(ASR_SAMPLE_RATE as i32, samples);
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
    }

    /// Current hypothesis; empty until speech is decoded.
    pub fn partial(&self) -> String {
        self.recognizer
            .get_result(&self.stream)
            .map(|r| r.text)
            .unwrap_or_default()
    }

    /// Whether trailing silence marked an utterance boundary.
    pub fn is_endpoint(&self) -> bool {
        self.recognizer.is_endpoint(&self.stream)
    }

    /// Finalize the current utterance (empty if silence-only) and start a fresh one.
    pub fn take_final(&self) -> String {
        let text = self.partial();
        self.recognizer.reset(&self.stream);
        text
    }

    /// Signal end of input and return any trailing speech not yet finalized.
    pub fn finish(&self) -> String {
        self.stream.input_finished();
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
        let text = self.partial();
        self.recognizer.reset(&self.stream);
        text
    }
}

pub(crate) fn path_to_string(path: &Path) -> Result<String, AsrError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(AsrError::NonUtf8Path)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Assets come from `scripts/fetch-assets.sh`; tests skip when absent (CI).
    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn assets_present() -> bool {
        let root = repo_root();
        let model = root.join("assets/models/sherpa-onnx-whisper-base.en");
        let wav = root.join("assets/wavs/1.wav");
        super::model_present(&model) && wav.exists()
    }

    #[test]
    fn loads_model_and_transcribes_known_wav() {
        if !assets_present() {
            eprintln!("SKIP: run `scripts/fetch-assets.sh` to download the Whisper model + WAV");
            return;
        }
        let root = repo_root();
        let model_dir = root.join("assets/models/sherpa-onnx-whisper-base.en");
        let wav = root.join("assets/wavs/1.wav");
        let engine = super::Engine::from_model_dir(&model_dir).expect("load model");
        assert!(engine.is_whisper(), "expected the Whisper engine to be selected");
        let transcript = engine.transcribe_wav(&wav).expect("transcribe wav");

        let text = transcript.text.to_lowercase();
        assert!(
            text.contains("direct consequence"),
            "expected distinctive phrase in transcript: {:?}",
            transcript.text
        );
        assert!(
            text.contains("blessed soul in heaven"),
            "expected tail phrase in transcript: {:?}",
            transcript.text
        );
    }

    /// Windowed feeding must reproduce the whole-file transcript.
    #[test]
    fn whisper_transcribes_wav_in_windows() {
        if !assets_present() {
            eprintln!("SKIP: run `scripts/fetch-assets.sh` to download the Whisper model + WAV");
            return;
        }
        let root = repo_root();
        let model_dir = root.join("assets/models/sherpa-onnx-whisper-base.en");
        let wav = root.join("assets/wavs/1.wav");
        let engine = super::whisper::WhisperEngine::new(
            super::whisper::WhisperModelConfig::from_model_dir(&model_dir).expect("model config"),
        )
        .expect("load engine");

        let wave = Wave::read(wav.to_str().unwrap()).expect("read wav");
        let rate = wave.sample_rate();
        let mut parts = Vec::new();
        for chunk in wave.samples().chunks(super::whisper::WINDOW_SAMPLES) {
            if let Ok(text) = engine.transcribe(rate as u32, chunk) {
                parts.push(text);
            }
        }
        let text = parts.join(" ").to_lowercase();
        assert!(
            text.contains("direct consequence"),
            "expected distinctive phrase in windowed transcript: {:?}",
            text
        );
    }
}
