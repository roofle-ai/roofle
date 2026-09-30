//! Silero VAD segmentation for the offline Whisper path.
//!
//! Raw fixed-length windows of silent or noisy audio make Whisper loop and
//! hallucinate filler, so each speech utterance is handed to it as its own clip.

use std::path::Path;

use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use super::AsrError;

pub const VAD_SAMPLE_RATE: u32 = 16_000;

/// Turns a raw audio stream into discrete speech segments.
pub struct Vad {
    detector: VoiceActivityDetector,
}

impl Vad {
    pub fn new(model: impl AsRef<Path>) -> Result<Self, AsrError> {
        let path = super::path_to_string(model.as_ref())?;
        let mut config = VadModelConfig::default();
        config.silero_vad = SileroVadModelConfig {
            model: Some(path),
            // Below the 0.5 default: real mic audio is quieter than test WAVs,
            // and a higher threshold drops speech entirely.
            threshold: 0.35,
            min_silence_duration: 0.4,
            min_speech_duration: 0.2,
            // Stay well under Whisper's ~30 s context.
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

    pub fn push(&self, samples: &[f32]) {
        self.detector.accept_waveform(samples);
    }

    pub fn drain(&self) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        while let Some(segment) = self.detector.front() {
            let samples = segment.samples().to_vec();
            self.detector.pop();
            out.push(samples);
        }
        out
    }

    pub fn flush(&self) -> Vec<Vec<f32>> {
        self.detector.flush();
        self.drain()
    }
}
