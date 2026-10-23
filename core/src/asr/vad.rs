//! Silero VAD segmentation for the offline Whisper path.
//!
//! Raw fixed-length windows of silent or noisy audio make Whisper loop and
//! hallucinate filler, so each speech utterance is handed to it as its own clip.
//! The detector trims tightly, which clips soft word onsets and endings, so each
//! clip is widened with a little of the real audio around it.

use std::ops::Range;
use std::path::Path;

use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use super::AsrError;

pub const VAD_SAMPLE_RATE: u32 = 16_000;

/// Real audio added before and after each detected utterance.
const PAD_SAMPLES: usize = VAD_SAMPLE_RATE as usize / 5; // 0.2 s

/// Longest utterance before the detector force-splits it. Kept short so long
/// stretches of talk reach the screen in pieces; well under Whisper's ~30 s context.
const MAX_SPEECH_SECS: f32 = 10.0;

/// An utterance this long was cut by the limit, not by a pause.
const FORCED_SPLIT_SAMPLES: usize = ((MAX_SPEECH_SECS - 0.5) * VAD_SAMPLE_RATE as f32) as usize;

/// Audio kept so padding can reach back before an utterance.
const HISTORY_SAMPLES: usize = 30 * VAD_SAMPLE_RATE as usize;

/// Turns a raw audio stream into discrete speech segments.
pub struct Vad {
    detector: VoiceActivityDetector,
    /// The most recent audio, ending at `total`.
    history: Vec<f32>,
    /// Stream position of `history[0]`.
    history_start: usize,
    /// Samples pushed so far.
    total: usize,
    /// Where the previous utterance's speech ended; padding never reaches back past it.
    prev_end: usize,
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
            // Short pauses end an utterance sooner, so text appears sooner.
            min_silence_duration: 0.25,
            // Short enough to keep one-word answers such as "yes" or "okay".
            min_speech_duration: 0.15,
            max_speech_duration: MAX_SPEECH_SECS,
            window_size: 512,
        };
        config.sample_rate = VAD_SAMPLE_RATE as i32;
        config.num_threads = 1;
        config.provider = Some("cpu".to_string());

        let detector = VoiceActivityDetector::create(&config, 60.0).ok_or_else(|| {
            AsrError::Create("could not create the Silero VAD".into())
        })?;
        Ok(Self { detector, history: Vec::new(), history_start: 0, total: 0, prev_end: 0 })
    }

    pub fn push(&mut self, samples: &[f32]) {
        self.detector.accept_waveform(samples);
        self.history.extend_from_slice(samples);
        self.total += samples.len();
        // Trim in bulk so the copy happens rarely, not on every block.
        if self.history.len() > 2 * HISTORY_SAMPLES {
            let excess = self.history.len() - HISTORY_SAMPLES;
            self.history.drain(..excess);
            self.history_start += excess;
        }
    }

    /// Whether speech is happening right now, as of the last block pushed.
    pub fn speaking(&self) -> bool {
        self.detector.detected()
    }

    /// Finished utterances, each widened with the audio around it.
    pub fn drain(&mut self) -> Vec<Vec<f32>> {
        let mut out = Vec::new();
        while let Some(segment) = self.detector.front() {
            let start = segment.start().max(0) as usize;
            let speech = segment.samples().to_vec();
            self.detector.pop();
            out.push(self.widen(start, speech));
        }
        out
    }

    pub fn flush(&mut self) -> Vec<Vec<f32>> {
        self.detector.flush();
        self.drain()
    }

    /// Add padding from the audio history around `speech`, which begins at `start`.
    fn widen(&mut self, start: usize, speech: Vec<f32>) -> Vec<f32> {
        let forced = speech.len() >= FORCED_SPLIT_SAMPLES;
        let range = padded_range(start, speech.len(), self.history_start, self.total, self.prev_end, forced);
        self.prev_end = start + speech.len();
        match range {
            Some(r) => self.history[r.start - self.history_start..r.end - self.history_start].to_vec(),
            None => speech,
        }
    }
}

/// The stream range to decode for speech at `start..start + len`: padded on both
/// sides, but never into the previous utterance, never past the audio received,
/// and not at the end of a forced split (the next piece continues from there).
/// `None` when the speech has already left the history.
fn padded_range(
    start: usize,
    len: usize,
    history_start: usize,
    total: usize,
    prev_end: usize,
    forced: bool,
) -> Option<Range<usize>> {
    let end = start + len;
    if start < history_start || end > total {
        return None;
    }
    let from = start.saturating_sub(PAD_SAMPLES).max(history_start).max(prev_end.min(start));
    let to = if forced { end } else { (end + PAD_SAMPLES).min(total) };
    Some(from..to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_both_sides() {
        assert_eq!(padded_range(10_000, 8_000, 0, 50_000, 0, false), Some(6_800..21_200));
    }

    #[test]
    fn padding_stops_at_the_previous_utterance() {
        assert_eq!(padded_range(10_000, 8_000, 0, 50_000, 9_000, false), Some(9_000..21_200));
    }

    #[test]
    fn padding_stops_at_the_audio_received() {
        assert_eq!(padded_range(10_000, 8_000, 0, 19_000, 0, false), Some(6_800..19_000));
    }

    #[test]
    fn padding_stops_at_the_oldest_kept_audio() {
        assert_eq!(padded_range(10_000, 8_000, 9_000, 50_000, 0, false), Some(9_000..21_200));
    }

    #[test]
    fn forced_split_gets_no_trailing_padding() {
        assert_eq!(padded_range(10_000, 8_000, 0, 50_000, 0, true), Some(6_800..18_000));
    }

    #[test]
    fn speech_gone_from_history_is_not_padded() {
        assert_eq!(padded_range(1_000, 8_000, 5_000, 50_000, 0, false), None);
    }
}
