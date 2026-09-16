//! Silero VAD segmentation for the offline Whisper path.
//!
//! Feeding Whisper raw fixed-length windows of silence-heavy, noise-laden or
//! echo-laden audio is the main cause of its repetitive "looping" / filler
//! hallucination. The robust routing — the same one the original Roofle used
//! (`VAD_ENABLED=true`) — is to run the incoming audio through Silero VAD and
//! hand each clean speech utterance to Whisper as a standalone clip, dropping
//! the silence in between.

use std::path::Path;

use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use super::AsrError;

/// Sample rate (Hz) the VAD consumes; matches the pipeline's 16 kHz mono audio.
pub const VAD_SAMPLE_RATE: u32 = 16_000;

/// A Silero voice-activity detector that turns a raw audio stream into discrete
/// speech segments.
pub struct Vad {
    detector: VoiceActivityDetector,
}

impl Vad {
    /// Load the Silero VAD ONNX model (`silero_vad.onnx`).
    pub fn new(model: impl AsRef<Path>) -> Result<Self, AsrError> {
        let path = super::path_to_string(model.as_ref())?;
        let mut config = VadModelConfig::default();
        config.silero_vad = SileroVadModelConfig {
            model: Some(path),
            // 0.35 is more forgiving than the default 0.5: real microphone audio
            // is often quieter than clean test WAVs, and a too-high threshold
            // drops speech entirely (silence never reaches Whisper).
            threshold: 0.35,
            min_silence_duration: 0.4,
            min_speech_duration: 0.2,
            // Cap each utterance well under Whisper's ~30 s context.
            max_speech_duration: 20.0,
            window_size: 512,
        };
        config.sample_rate = VAD_SAMPLE_RATE as i32;
        config.num_threads = 1;
        config.provider = Some("cpu".to_string());

        let detector = VoiceActivityDetector::create(&config, 60.0).ok_or_else(|| {
            AsrError::Create("could not create the Silero VAD".into())
        })?;
        Ok(Self { detector })
    }

    /// Feed 16 kHz mono samples into the detector.
    pub fn push(&self, samples: &[f32]) {
        self.detector.accept_waveform(samples);
    }

    /// Pop every completed speech segment as a standalone sample buffer.
    pub fn drain(&self) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        while let Some(segment) = self.detector.front() {
            let samples = segment.samples().to_vec();
            self.detector.pop();
            out.push(samples);
        }
        out
    }

    /// Flush any trailing buffered speech, then drain it.
    pub fn flush(&self) -> Vec<Vec<f32>> {
        self.detector.flush();
        self.drain()
    }
}
