//! Audio processing: normalize captured audio to 16 kHz mono `f32`.
//!
//! S4 slice. Per `design.md` §3, every source (mic, system audio) is normalized
//! at the capture boundary with three pure operations:
//!
//! - [`downmix_to_mono`] — average interleaved channels into one stream
//! - [`Resampler`] — streaming resample to [`TARGET_SAMPLE_RATE`] (16 kHz) via
//!   rubato's band-limited sinc interpolation
//! - [`apply_gain`] — optional amplitude gain, output clamped to avoid clipping
//!
//! The [`Resampler`] is a thin streaming wrapper around [`SincFixedIn`]: rubato's
//! fixed-input resamplers require exactly `input_frames_next()` frames per call,
//! so we buffer variable-size input chunks until a full block is available,
//! keeping the API suitable for a real-time capture thread (never blocks).

use std::error::Error;
use std::fmt;

use rubato::{
    ResampleError, Resampler as RubatoResampler, SincFixedIn, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};

/// Fixed sample rate (Hz) every source is resampled to before ASR.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Sinc interpolation filter length (samples); 256 is a good quality/cost
/// starting point for speech (rubato docs).
const SINC_LEN: usize = 256;
/// Relative low-pass cutoff for the anti-aliasing filter.
const F_CUTOFF: f32 = 0.95;
/// Number of intermediate points used by the interpolator.
const OVERSAMPLING: usize = 128;
/// Minimum input block (frames) per rubato process call. Must stay comfortably
/// above `SINC_LEN` so rubato's internal `end_idx` stays positive even at low
/// input rates (e.g. 8 kHz → 10 ms is only 80 frames, too small).
const MIN_CHUNK_FRAMES: usize = 512;

/// Errors from audio processing.
#[derive(Debug)]
pub enum ProcessingError {
    /// The source sample rate is not usable (zero).
    InvalidRate(u32),
    /// Failed to construct the resampler for the requested rate.
    ResamplerConstruction(String),
    /// A runtime resampling step failed.
    Resample(String),
}

impl fmt::Display for ProcessingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessingError::InvalidRate(r) => {
                write!(f, "invalid source sample rate for processing: {r}")
            }
            ProcessingError::ResamplerConstruction(e) => {
                write!(f, "failed to build resampler: {e}")
            }
            ProcessingError::Resample(e) => write!(f, "resampling failed: {e}"),
        }
    }
}

impl Error for ProcessingError {}

impl From<rubato::ResamplerConstructionError> for ProcessingError {
    fn from(e: rubato::ResamplerConstructionError) -> Self {
        ProcessingError::ResamplerConstruction(e.to_string())
    }
}

impl From<ResampleError> for ProcessingError {
    fn from(e: ResampleError) -> Self {
        ProcessingError::Resample(e.to_string())
    }
}

/// Average interleaved channels into a single mono stream.
///
/// Channels are summed and divided by the channel count (a "soft" downmix).
/// A mono input (`channels <= 1`) is passed through untouched.
pub fn downmix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    let frames = samples.len() / channels;
    let mut mono = Vec::with_capacity(frames);
    for frame in samples.chunks_exact(channels) {
        let sum: f32 = frame.iter().sum();
        mono.push(sum / channels as f32);
    }
    mono
}

/// Apply an optional amplitude gain, clamping the result to `[-1.0, 1.0]` so a
/// loud source cannot hard-clip into non-finite samples.
///
/// A non-finite or non-positive gain is ignored (returns the buffer unchanged).
pub fn apply_gain(samples: &mut [f32], gain: f32) {
    if !gain.is_finite() || gain <= 0.0 {
        return;
    }
    for s in samples.iter_mut() {
        *s = (*s * gain).clamp(-1.0, 1.0);
    }
}

/// Streaming resampler from an arbitrary source rate to [`TARGET_SAMPLE_RATE`]
/// (16 kHz) mono.
///
/// Wraps rubato [`SincFixedIn`], buffering whatever sized mono chunks arrive on
/// the capture thread until a full block is available, then emitting the
/// resampled 16 kHz mono output. The capture callback only feeds samples in;
/// all heavy work happens here (and, later, on the ASR thread).
pub struct Resampler {
    inner: SincFixedIn<f32>,
    input_rate: u32,
    /// Mono input frames buffered but not yet resampled.
    pending: Vec<f32>,
    /// Reusable input block of `chunk` frames.
    in_buf: Vec<f32>,
    /// Reusable output buffer, sized `output_frames_next()`.
    out_buf: Vec<f32>,
    /// Input frames consumed per rubato process call (`input_frames_next()`).
    chunk: usize,
}

impl Resampler {
    /// Build a resampler that converts mono audio at `input_rate` Hz to
    /// [`TARGET_SAMPLE_RATE`].
    pub fn new(input_rate: u32) -> Result<Self, ProcessingError> {
        if input_rate == 0 {
            return Err(ProcessingError::InvalidRate(input_rate));
        }
        // rubato's `resample_ratio` is output-rate/input-rate.
        let ratio = TARGET_SAMPLE_RATE as f64 / input_rate as f64;
        let params = SincInterpolationParameters {
            sinc_len: SINC_LEN,
            f_cutoff: F_CUTOFF,
            oversampling_factor: OVERSAMPLING,
            interpolation: SincInterpolationType::Cubic,
            window: WindowFunction::BlackmanHarris2,
        };
        // ~10 ms blocks, but never below the sinc-safe minimum.
        let chunk = (input_rate as usize / 100).max(MIN_CHUNK_FRAMES);
        let inner = SincFixedIn::<f32>::new(ratio, 1.0, params, chunk, 1)?;
        let in_buf = vec![0.0; inner.input_frames_next()];
        let out_buf = vec![0.0; inner.output_frames_next()];
        Ok(Self {
            inner,
            input_rate,
            pending: Vec::new(),
            in_buf,
            out_buf,
            chunk,
        })
    }

    /// Source sample rate this resampler was configured for.
    pub fn input_rate(&self) -> u32 {
        self.input_rate
    }

    /// Output sample rate (always [`TARGET_SAMPLE_RATE`]).
    pub fn output_rate(&self) -> u32 {
        TARGET_SAMPLE_RATE
    }

    /// Push mono `f32` samples and get back any 16 kHz mono output produced.
    ///
    /// Returns an empty vec if fewer than a full internal block has accumulated.
    /// Never blocks; callers should feed whatever buffer the capture thread
    /// produced.
    pub fn process(&mut self, mono: &[f32]) -> Result<Vec<f32>, ProcessingError> {
        if !mono.is_empty() {
            self.pending.extend_from_slice(mono);
        }
        self.drain_pending()
    }

    /// Consume as many full input blocks as are buffered, emitting their
    /// resampled output. Shared by [`process`](Self::process) and
    /// [`flush`](Self::flush).
    fn drain_pending(&mut self) -> Result<Vec<f32>, ProcessingError> {
        let mut out = Vec::new();
        while self.pending.len() >= self.chunk {
            self.in_buf.copy_from_slice(&self.pending[..self.chunk]);
            let (_, produced) = self.inner.process_into_buffer(
                &[self.in_buf.as_slice()],
                &mut [self.out_buf.as_mut_slice()],
                None,
            )?;
            out.extend_from_slice(&self.out_buf[..produced]);
            self.pending.drain(..self.chunk);
        }
        Ok(out)
    }

    /// Flush the resampler tail after the source has stopped.
    ///
    /// rubato's fixed-input resamplers cannot process a partial block, so the
    /// remaining buffered frames are zero-padded to a full block and resampled.
    /// This yields the last fraction of a block (~10 ms) of output and clears
    /// all internal state. The trailing padded samples are silence, so they are
    /// harmless at the very end of a capture.
    pub fn flush(&mut self) -> Result<Vec<f32>, ProcessingError> {
        if self.pending.is_empty() {
            return Ok(Vec::new());
        }
        self.pending.resize(self.chunk, 0.0);
        self.drain_pending()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RMS of a sine of the given peak amplitude is `amp / sqrt(2)`.
    fn peak_rms(amp: f32) -> f32 {
        amp / std::f32::consts::SQRT_2
    }

    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }

    /// 1 kHz sine at the given rate, peak amplitude `amp`.
    fn sine(rate: u32, seconds: f32, amp: f32) -> Vec<f32> {
        let n = (rate as f32 * seconds) as usize;
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / rate as f32).sin())
            .collect()
    }

    // --- downmix ---------------------------------------------------------

    #[test]
    fn downmix_stereo_to_mono() {
        let s = downmix_to_mono(&[0.0, 1.0, 0.5, 0.5, 0.25, -0.25], 2);
        assert_eq!(s, vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn downmix_four_channels_averaged() {
        let s = downmix_to_mono(&[1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0], 4);
        assert_eq!(s, vec![1.0, 2.0]);
    }

    #[test]
    fn downmix_passthrough_mono() {
        let s = downmix_to_mono(&[0.1, 0.2, 0.3], 1);
        assert_eq!(s, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn downmix_ignores_partial_tail_frame() {
        // 5 samples with 2 channels -> 2 full frames; the trailing sample drops.
        let s = downmix_to_mono(&[0.0, 2.0, 0.5, 0.5, 0.25], 2);
        assert_eq!(s, vec![1.0, 0.5]);
    }

    // --- gain ------------------------------------------------------------

    #[test]
    fn gain_scales_and_clamps() {
        let mut s = [0.5, -0.5, 0.9, 1.0];
        apply_gain(&mut s, 2.0);
        assert_eq!(s, [1.0, -1.0, 1.0, 1.0]);
    }

    #[test]
    fn gain_ignores_nonpositive_or_nan() {
        let mut s = [0.5, 0.5];
        apply_gain(&mut s, 0.0);
        assert_eq!(s, [0.5, 0.5]);
        let mut s = [0.5, 0.5];
        apply_gain(&mut s, f32::NAN);
        assert_eq!(s, [0.5, 0.5]);
    }

    // --- resampler -------------------------------------------------------

    #[test]
    fn resampler_rejects_zero_rate() {
        assert!(matches!(
            Resampler::new(0),
            Err(ProcessingError::InvalidRate(0))
        ));
    }

    #[test]
    fn resample_silence_stays_silent() {
        let mut r = Resampler::new(44_100).unwrap();
        let out = r.process(&vec![0.0; 44_100]).unwrap();
        assert!(out.iter().all(|&s| s.abs() < 1e-4), "silence leaked energy");
    }

    #[test]
    fn resample_16k_passthrough_length_and_rms() {
        let rate = 16_000;
        let mut r = Resampler::new(rate).unwrap();
        let input = sine(rate, 1.0, 0.5);
        let mut out = r.process(&input).unwrap();
        out.extend(r.flush().unwrap());
        // Roughly 1 s at 16 kHz (per-block rounding + filter delay).
        let diff = (out.len() as i64 - 16_000).unsigned_abs();
        assert!(diff < 400, "unexpected output length {}", out.len());
        // A 1 kHz tone is far below the cutoff, amplitude is preserved.
        assert!(
            (rms(&out) - peak_rms(0.5)).abs() < 0.03,
            "rms {}",
            rms(&out)
        );
    }

    #[test]
    fn resample_48k_to_16k_third_the_length() {
        let rate = 48_000;
        let mut r = Resampler::new(rate).unwrap();
        let input = sine(rate, 1.0, 0.5);
        let mut out = r.process(&input).unwrap();
        out.extend(r.flush().unwrap());
        // 1 s @48k -> ~1 s @16k, i.e. ~1/3 of the input length.
        let expected = input.len() / 3;
        let diff = (out.len() as i64 - expected as i64).unsigned_abs();
        assert!(diff < 400, "expected ~{expected}, got {}", out.len());
        assert!(
            (rms(&out) - peak_rms(0.5)).abs() < 0.03,
            "rms {}",
            rms(&out)
        );
    }

    #[test]
    fn resample_8k_to_16k_doubles_length() {
        let rate = 8_000;
        let mut r = Resampler::new(rate).unwrap();
        let input = sine(rate, 1.0, 0.5);
        let mut out = r.process(&input).unwrap();
        out.extend(r.flush().unwrap());
        let expected = input.len() * 2;
        let diff = (out.len() as i64 - expected as i64).unsigned_abs();
        assert!(diff < 400, "expected ~{expected}, got {}", out.len());
    }

    #[test]
    fn resampler_buffers_variable_input() {
        // Feed the same 1 s of 44.1 kHz audio as one block vs many uneven
        // blocks; both must yield ~the same number of output frames.
        let rate = 44_100;
        let input = sine(rate, 1.0, 0.3);

        let mut whole = Resampler::new(rate).unwrap();
        let out_whole = whole.process(&input).unwrap();

        let mut split = Resampler::new(rate).unwrap();
        let mut out_split = Vec::new();
        for chunk in input.chunks(997) {
            out_split.extend(split.process(chunk).unwrap());
        }
        let diff = (out_whole.len() as i64 - out_split.len() as i64).unsigned_abs();
        assert!(
            diff < 100,
            "whole {} vs split {}",
            out_whole.len(),
            out_split.len()
        );
    }

    #[test]
    fn flush_drains_pending_tail() {
        let rate = 16_000;
        let mut r = Resampler::new(rate).unwrap();
        // 1000 frames with a 512-frame block -> 488 buffered tail.
        r.process(&vec![0.1; 1000]).unwrap();
        assert!(
            r.pending.len() < r.chunk,
            "pending not drained: {}",
            r.pending.len()
        );
        let tail = r.flush().unwrap();
        assert!(r.pending.is_empty(), "flush must clear pending");
        assert!(!tail.is_empty(), "flush should emit the buffered tail");
        // Tail of a constant +0.1 signal stays low-energy / well-behaved.
        assert!(tail.iter().all(|&s| s.is_finite() && s.abs() < 0.5));

        // The resampler is reusable after a flush: a fresh sine still resamples
        // at the correct rate. Skip the first `delay` frames of filter pre-roll.
        let input = sine(rate, 1.0, 0.4);
        let mut out = r.process(&input).unwrap();
        out.extend(r.flush().unwrap());
        let diff = (out.len() as i64 - 16_000).unsigned_abs();
        assert!(diff < 500, "reused resampler output length {}", out.len());
    }
}
