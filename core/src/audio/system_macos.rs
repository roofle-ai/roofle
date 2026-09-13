//! macOS system-audio capture via Apple's ScreenCaptureKit (S11).
//!
//! Captures the system's audio output (everything the user hears) through the
//! pure-Rust `screencapturekit` crate — no Swift/Obj-C shim. Audio arrives as
//! `CMSampleBuffer`s on a ScreenCaptureKit-owned callback queue; each is
//! downmixed to mono at its native rate and sent as a [`Frame`] down the same
//! channel shape the microphone uses. The pipeline's shared `pump_loop`
//! resamples those frames to 16 kHz mono. The callback never blocks: it only
//! converts and pushes frames.
//!
//! ```text
//! system audio --SCStream callback--> CMSampleBuffer --downmix--> Frame --pump_loop--> ring
//! ```
//!
//! ## Permission
//!
//! Screen Recording is a separate TCC category from the microphone. Accessing
//! shareable content fails until the user grants it
//! (`NSScreenCaptureUsageDescription`; see `design.md`). [`SystemSource::start`]
//! surfaces that failure as a [`SourceError`](crate::pipeline::SourceError).

use std::sync::mpsc::Sender;

use screencapturekit::prelude::*;

use crate::audio::mic::Frame;
use crate::pipeline::{Ring, Source, SourceError};

/// System audio is delivered at this rate by ScreenCaptureKit after our config.
const SYSTEM_SAMPLE_RATE: u32 = 48_000;

/// Audio output stream that sends native-rate mono `Frame`s to a channel.
///
/// Mirrors the microphone capture (`audio::mic`) so the pipeline's shared
/// `pump_loop` can drive it unchanged: this struct owns the live `SCStream`
/// (not `Send`, so it stays on the pipeline thread) and delivers downmixed mono
/// `Frame`s down the same `mpsc` channel at the native sample rate.
pub struct SystemCapture {
    // Held for its Drop side-effect: dropping the SCStream stops capture.
    #[allow(dead_code)]
    stream: Option<SCStream>,
    sample_rate: u32,
    channels: u16,
}

impl SystemCapture {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }
}

/// Open the system-audio stream and send downmixed mono frames to `tx`.
pub fn open(tx: Sender<Frame>) -> Result<SystemCapture, SystemError> {
    // Fails with a permission error until Screen Recording is granted.
    let content = SCShareableContent::get().map_err(|e| SystemError::Permission(e.to_string()))?;
    let display = content
        .displays()
        .first()
        .ok_or_else(|| SystemError::NoDisplay)?
        .clone();

    let filter = SCContentFilter::create()
        .with_display(&display)
        .with_excluding_windows(&[])
        .build();
    let config = SCStreamConfiguration::new()
        .with_captures_audio(true)
        .with_sample_rate(SYSTEM_SAMPLE_RATE as i32)
        .with_channel_count(2);

    let mut stream = SCStream::new(&filter, &config);

    let tx = tx.clone();
    stream.add_output_handler(
        move |sample, output_type| {
            if output_type == SCStreamOutputType::Audio {
                if let Some(frame) = to_frame(sample) {
                    let _ = tx.send(frame);
                }
            }
        },
        SCStreamOutputType::Audio,
    );

    stream
        .start_capture()
        .map_err(|e| SystemError::Start(e.to_string()))?;

    Ok(SystemCapture {
        stream: Some(stream),
        sample_rate: SYSTEM_SAMPLE_RATE,
        channels: 2,
    })
}

/// Convert one audio `CMSampleBuffer` into a downmixed mono [`Frame`].
///
/// Kept cheap (decode + downmix + RMS) since it runs on the capture callback.
fn to_frame(sample: CMSampleBuffer) -> Option<Frame> {
    let fmt = sample.format_description()?;
    let rate = fmt.audio_sample_rate()? as u32;
    let list = sample.audio_buffer_list()?;

    let float = fmt.audio_is_float();
    let bits = fmt.audio_bits_per_channel().unwrap_or(32) as usize;
    let bytes_per_sample = (bits / 8).max(1);
    let channels = fmt.audio_channel_count().unwrap_or(1) as usize;

    let buffers: Vec<_> = list.iter().collect();
    let samples = if buffers.len() > 1 {
        // Planar: one buffer per channel.
        let frames = buffers[0].data().len() / bytes_per_sample;
        let mut samples = Vec::with_capacity(frames);
        for i in 0..frames {
            let mut sum = 0.0f64;
            for b in &buffers {
                let data = b.data();
                if let Some(s) = decode(&data[i * bytes_per_sample..], bytes_per_sample, float) {
                    sum += s as f64;
                }
            }
            samples.push((sum / buffers.len() as f64) as f32);
        }
        samples
    } else if let Some(b) = buffers.first() {
        // Interleaved in one buffer.
        let data = b.data();
        let ch = b.number_channels.max(1) as usize;
        let total = data.len() / bytes_per_sample;
        let frames = total / ch;
        let mut samples = Vec::with_capacity(frames);
        for f in 0..frames {
            let mut sum = 0.0f64;
            for c in 0..ch {
                let idx = (f * ch + c) * bytes_per_sample;
                if let Some(s) = decode(&data[idx..], bytes_per_sample, float) {
                    sum += s as f64;
                }
            }
            samples.push((sum / ch as f64) as f32);
        }
        samples
    } else {
        return None;
    };

    let rms = rms_of(&samples);
    Some(Frame {
        samples,
        sample_rate: rate,
        channels: channels as u16,
        rms,
    })
}

/// Decode one little-endian PCM sample from `bytes` into `f32`.
fn decode(bytes: &[u8], bytes_per_sample: usize, float: bool) -> Option<f32> {
    if bytes.len() < bytes_per_sample {
        return None;
    }
    let b = &bytes[..bytes_per_sample];
    if float && bytes_per_sample >= 4 {
        let mut raw = [0u8; 4];
        raw.copy_from_slice(&b[..4]);
        return Some(f32::from_le_bytes(raw));
    }
    if bytes_per_sample >= 2 {
        return Some(i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0);
    }
    None
}

/// Root-mean-square amplitude of a mono buffer.
fn rms_of(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// A [`Source`] that feeds the pipeline from the system's audio output.
///
/// Owns the [`SystemCapture`] (and its non-`Send` `SCStream`) on the pipeline
/// thread and delegates frame pumping/resampling to the shared `pump_loop`,
/// exactly like `pipeline::MicSource`.
pub struct SystemSource {
    capture: Option<SystemCapture>,
    thread: Option<std::thread::JoinHandle<()>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// When true the pump discards captured audio instead of feeding the ring.
    paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
    gain: f32,
}

impl SystemSource {
    pub fn new(gain: f32) -> Self {
        Self {
            capture: None,
            thread: None,
            stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            paused: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            gain,
        }
    }
}

impl Source for SystemSource {
    fn start(&mut self, ring: Ring) -> Result<(), SourceError> {
        let (tx, rx) = std::sync::mpsc::channel::<Frame>();
        let capture = open(tx)?;
        let sample_rate = capture.sample_rate();
        let gain = self.gain;
        let stop = self.stop.clone();
        let paused = self.paused.clone();
        let ring = ring.clone();
        self.thread = Some(std::thread::spawn(move || {
            pump_loop(rx, sample_rate, gain, ring, stop, paused);
        }));
        self.capture = Some(capture);
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        // Dropping the SCStream stops capture and disconnects the pump's
        // receiver — a second signal for the pump thread to exit.
        self.capture = None;
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }

    fn set_paused(&mut self, paused: bool) {
        self.paused.store(paused, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Resample native-rate system-audio frames to 16 kHz mono and push them into
/// the ring. Runs on the source's pump thread — never the capture callback.
fn pump_loop(
    frames: std::sync::mpsc::Receiver<Frame>,
    sample_rate: u32,
    gain: f32,
    ring: Ring,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    let mut resampler = match crate::audio::processing::Resampler::new(sample_rate) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[roofle-core] system pump resampler: {e}");
            return;
        }
    };
    let poll = std::time::Duration::from_millis(100);
    while !stop.load(std::sync::atomic::Ordering::SeqCst) && !ring.is_closed() {
        match frames.recv_timeout(poll) {
            Ok(frame) => {
                // While paused, keep draining the capture channel so the capture
                // callback never blocks, but drop the samples before they reach
                // the ring (and the ASR thread).
                if paused.load(std::sync::atomic::Ordering::SeqCst) {
                    continue;
                }
                let mut mono = match resampler.process(&frame.samples) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("[roofle-core] system pump resample: {e}");
                        break;
                    }
                };
                if gain != 1.0 {
                    crate::audio::processing::apply_gain(&mut mono, gain);
                }
                if !mono.is_empty() {
                    ring.push(&mono);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if let Ok(tail) = resampler.flush() {
        if !tail.is_empty() {
            ring.push(&tail);
        }
    }
}

/// Errors from system-audio capture.
#[derive(Debug)]
pub enum SystemError {
    /// Screen Recording permission not granted, or content unavailable.
    Permission(String),
    /// No display to attach the stream to.
    NoDisplay,
    /// Failed to start the capture stream.
    Start(String),
}

impl std::fmt::Display for SystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemError::Permission(e) => {
                write!(f, "screen recording permission denied: {e}")
            }
            SystemError::NoDisplay => write!(f, "no display available for system audio"),
            SystemError::Start(e) => write!(f, "failed to start system audio capture: {e}"),
        }
    }
}

impl std::error::Error for SystemError {}

impl From<SystemError> for SourceError {
    fn from(e: SystemError) -> Self {
        SourceError::System(e.to_string())
    }
}
