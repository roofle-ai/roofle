//! Speech-to-text engine backed by the official `sherpa-onnx` Rust bindings.

pub mod segments;
pub mod vad;
pub mod whisper;

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, OnlineStream, Wave};

pub use segments::{Segment, SegmentKind, Transcript};

/// Sample rate (Hz) the streaming transducer models consume at their input.
/// Every capture source is normalized to this before ASR (see
/// `audio::processing::TARGET_SAMPLE_RATE`, which equals this value).
pub const ASR_SAMPLE_RATE: u32 = 16_000;

/// Leading silence (frames at [`ASR_SAMPLE_RATE`]) prepended before whole-file
/// transcription so the streaming recognizer has context and does not drop the
/// first ~0.3 s of speech. ~0.7 s at 16 kHz (chosen empirically so the known
/// test WAV transcribes cleanly from first to last word).
const WARM_UP_FRAMES: usize = (ASR_SAMPLE_RATE as usize) * 7 / 10;

/// Errors produced by the ASR engine.
#[derive(Debug)]
pub enum AsrError {
    /// Failed to build the recognizer from the supplied model files.
    Create(String),
    /// Failed to read a WAV file.
    ReadWave(String),
    /// The recognizer produced no text for the given audio.
    EmptyTranscript,
    /// The input path is not valid UTF-8 (sherpa-onnx needs a `&str` path).
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
///
/// `from_model_dir` resolves the standard file names used by the development
/// model (`sherpa-onnx-streaming-zipformer-en-2023-06-26`); see
/// `scripts/fetch-assets.sh` to download it. Individual files can be overridden
/// on the returned struct.
#[derive(Debug, Clone)]
pub struct AsrModelConfig {
    /// ONNX encoder model (`encoder-*.int8.onnx`).
    pub encoder: PathBuf,
    /// ONNX decoder model (`decoder-*.int8.onnx`).
    pub decoder: PathBuf,
    /// ONNX joiner model (`joiner-*.int8.onnx`).
    pub joiner: PathBuf,
    /// BPE token vocabulary (`tokens.txt`).
    pub tokens: PathBuf,
    /// ONNX Runtime execution provider (e.g. `"cpu"`).
    pub provider: String,
    /// Number of inference threads (keep modest; ASR runs off the audio thread).
    pub num_threads: i32,
    /// Whether to detect utterance boundaries automatically.
    pub enable_endpoint: bool,
}

impl AsrModelConfig {
    /// Standard file names inside a downloaded streaming-zipformer model dir.
    pub const ENCODER_FILE: &'static str = "encoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx";
    pub const DECODER_FILE: &'static str = "decoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx";
    pub const JOINER_FILE: &'static str = "joiner-epoch-99-avg-1-chunk-16-left-128.int8.onnx";
    pub const TOKENS_FILE: &'static str = "tokens.txt";

    /// Resolve a config from a model directory containing the standard files.
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

    /// Whether all referenced files exist on disk.
    pub fn present_files(&self) -> bool {
        self.encoder.is_file()
            && self.decoder.is_file()
            && self.joiner.is_file()
            && self.tokens.is_file()
    }
}

/// The ASR engine for a model directory, auto-selected by its file layout.
///
/// A directory containing `<name>-encoder*.onnx` is treated as an exported
/// Whisper model ([`whisper::WhisperEngine`]); otherwise it is treated as a
/// streaming Zipformer-Transducer ([`AsrEngine`]).
pub enum Engine {
    /// Streaming Zipformer-Transducer engine (live partials).
    Streaming(AsrEngine),
    /// Offline Whisper engine (windowed final-only segments).
    Whisper(whisper::WhisperEngine),
}

impl Engine {
    /// Load whichever engine the model directory contains.
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

    /// Whether this is the offline Whisper engine.
    pub fn is_whisper(&self) -> bool {
        matches!(self, Engine::Whisper(_))
    }

    /// Sample rate (Hz) the engine expects at its input.
    pub fn sample_rate(&self) -> u32 {
        match self {
            Engine::Streaming(e) => e.sample_rate(),
            Engine::Whisper(e) => e.sample_rate(),
        }
    }

    /// Transcribe a WAV file (whole-file; Whisper chunks into ~28 s windows).
    pub fn transcribe_wav<P: AsRef<Path>>(&self, path: P) -> Result<Transcript, AsrError> {
        match self {
            Engine::Streaming(e) => e.transcribe_wav(path),
            Engine::Whisper(e) => e.transcribe_wav(path),
        }
    }
}

/// Whether every file of the model in `dir` is present on disk.
///
/// Works for both Whisper and streaming-transducer layouts.
pub fn model_present(dir: &Path) -> bool {
    if whisper::WhisperModelConfig::probe(dir) {
        whisper::WhisperModelConfig::from_model_dir(dir)
            .map(|c| c.encoder.is_file() && c.decoder.is_file() && c.tokens.is_file())
            .unwrap_or(false)
    } else {
        AsrModelConfig::from_model_dir(dir).present_files()
    }
}

/// A loaded streaming ASR engine (wraps `sherpa_onnx::OnlineRecognizer`).
///
/// Inference is CPU-bound and runs on the calling thread; keep that off the
/// real-time audio callback (plan: *never block the audio callback* — S7 moves
/// this to a dedicated thread fed by a ring buffer).
pub struct AsrEngine {
    recognizer: OnlineRecognizer,
}

impl AsrEngine {
    /// Load the model described by `config`.
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

    /// Sample rate (Hz) the model expects at its input ([`ASR_SAMPLE_RATE`]).
    pub fn sample_rate(&self) -> u32 {
        ASR_SAMPLE_RATE
    }

    /// Start a live streaming recognition session (S6).
    ///
    /// Prepends [`WARM_UP_FRAMES`] of silence so the recognizer has context and
    /// does not drop the first ~0.3 s of speech (same rationale as
    /// [`transcribe_waveform`](Self::transcribe_waveform)). Feed 16 kHz mono
    /// chunks with [`StreamingSession::feed`] and poll results.
    pub fn stream(&self) -> StreamingSession<'_> {
        let session = StreamingSession {
            recognizer: &self.recognizer,
            stream: self.recognizer.create_stream(),
        };
        let warm_up = vec![0.0f32; WARM_UP_FRAMES];
        session.feed(&warm_up);
        session
    }

    /// Transcribe a WAV file: read it, run the streaming recognizer over the
    /// whole waveform, and return the final [`Transcript`].
    pub fn transcribe_wav<P: AsRef<Path>>(&self, path: P) -> Result<Transcript, AsrError> {
        let path_str = path_to_string(path.as_ref())?;
        let wave = Wave::read(&path_str).ok_or_else(|| {
            AsrError::ReadWave(format!("could not read {path_str} (does the file exist?)"))
        })?;
        self.transcribe_waveform(wave.sample_rate() as u32, wave.samples())
    }

    /// Run the streaming recognizer over a complete 16 kHz mono waveform and
    /// return the final transcript.
    ///
    /// `samples` must be at the model's input rate ([`ASR_SAMPLE_RATE`]);
    /// resampling to that rate happens upstream (S4, `audio::processing`).
    ///
    /// A short silence warm-up is prepended because streaming recognizers drop
    /// the leading ~0.3 s of speech for lack of context.
    pub fn transcribe_waveform(
        &self,
        sample_rate: u32,
        samples: &[f32],
    ) -> Result<Transcript, AsrError> {
        let stream = self.recognizer.create_stream();
        let rate = sample_rate as i32;

        let mut audio = Vec::with_capacity(samples.len() + WARM_UP_FRAMES);
        audio.resize(WARM_UP_FRAMES, 0.0); // leading silence
        audio.extend_from_slice(samples);

        // Feed in ~0.2 s chunks with a decode between each. This mirrors live
        // streaming (S6) and is more robust at both ends of the utterance than
        // a single whole-file feed.
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

        // After input_finished the result text is the complete transcript.
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

/// A live streaming recognition session over the engine's stream (S6).
///
/// Created via [`AsrEngine::stream`]. Feed 16 kHz mono `f32` chunks with
/// [`feed`](StreamingSession::feed), poll [`partial`](StreamingSession::partial)
/// for the current hypothesis, and close out each utterance with
/// [`is_endpoint`](StreamingSession::is_endpoint) +
/// [`take_final`](StreamingSession::take_final). Call [`finish`](StreamingSession::finish)
/// once at shutdown to flush trailing speech.
///
/// All methods are `&self` (sherpa-onnx's `OnlineStream` takes shared refs), so
/// the session is cheap to drive from a single consumer thread — never from the
/// real-time audio callback.
pub struct StreamingSession<'a> {
    recognizer: &'a OnlineRecognizer,
    stream: OnlineStream,
}

impl StreamingSession<'_> {
    /// Sample rate (Hz) the session consumes ([`ASR_SAMPLE_RATE`]).
    pub fn sample_rate(&self) -> u32 {
        ASR_SAMPLE_RATE
    }

    /// Feed one chunk of 16 kHz mono samples and decode.
    ///
    /// Chunks can be any size; the recognizer accumulates them internally.
    pub fn feed(&self, samples: &[f32]) {
        self.stream.accept_waveform(ASR_SAMPLE_RATE as i32, samples);
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
    }

    /// Current hypothesis text (partial result); empty until speech is decoded.
    pub fn partial(&self) -> String {
        self.recognizer
            .get_result(&self.stream)
            .map(|r| r.text)
            .unwrap_or_default()
    }

    /// Whether the recognizer detected an utterance boundary (trailing silence).
    pub fn is_endpoint(&self) -> bool {
        self.recognizer.is_endpoint(&self.stream)
    }

    /// Finalize the current utterance and start a fresh one.
    ///
    /// Call when [`is_endpoint`](Self::is_endpoint) is true. Returns the
    /// finalized text (empty for a silence-only utterance).
    pub fn take_final(&self) -> String {
        let text = self.partial();
        self.recognizer.reset(&self.stream);
        text
    }

    /// Signal end of input and return the final result of the current utterance.
    ///
    /// Use once at shutdown to flush trailing speech the endpoint rules have not
    /// yet finalized.
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

    /// Dev model + test WAVs, downloaded by `scripts/fetch-assets.sh`. CI has no
    /// network models, so these tests skip (trivially pass) when absent.
    ///
    /// Paths are resolved from the crate dir (`core/`) up to the repo root so
    /// the test passes regardless of the invoking working directory.
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

        // Whisper is far more accurate than the old streaming Zipformer; it
        // reproduces the LibriSpeech test utterance essentially verbatim (with
        // punctuation). Assert on a stable phrase after normalizing case.
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

    /// The offline Whisper path must reproduce the whole-file transcript when
    /// the audio is fed in ~28 s windows (mirrors the live capture loop, which
    /// has no streaming hypotheses). No microphone is needed — the bundled WAV
    /// stands in for live audio.
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
