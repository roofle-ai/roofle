//! Microphone capture via [`cpal`].
//!
//! S3 slice: enumerate input devices, open the default (or a named) device,
//! and stream captured audio as mono `f32` frames plus an RMS level over a
//! [`std::sync::mpsc`] channel. Resampling to a fixed 16 kHz rate is deferred
//! to S4 (`audio::processing`), so frames carry the device's native sample
//! rate for now.
//!
//! The audio callback is never blocked: it only downmixes, computes RMS and
//! `try_send`s the frame. Heavy work happens on the consumer side.

use std::error::Error;
use std::fmt;
use std::sync::mpsc::Sender;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// A captured frame of audio, downmixed to mono `f32` at the device's native
/// sample rate, plus the RMS level of the frame (for a UI level meter).
#[derive(Debug, Clone)]
pub struct Frame {
    pub samples: Vec<f32>,
    /// Device's native sample rate (resample-to-16k lands in S4).
    pub sample_rate: u32,
    /// Source channel count before downmix.
    pub channels: u16,
    /// Root-mean-square amplitude of this frame, 0.0..=~1.0.
    pub rms: f32,
}

/// Errors that can occur while enumerating or opening a microphone.
#[derive(Debug)]
pub enum CaptureError {
    NoDevice,
    DeviceNotFound(String),
    DeviceName(String),
    StreamConfig(String),
    BuildStream(String),
    Play(String),
    Pause(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureError::NoDevice => write!(f, "no input (microphone) device available"),
            CaptureError::DeviceNotFound(n) => write!(f, "input device not found: {n}"),
            CaptureError::DeviceName(e) => write!(f, "failed to read device name: {e}"),
            CaptureError::StreamConfig(e) => write!(f, "failed to read input config: {e}"),
            CaptureError::BuildStream(e) => write!(f, "failed to build input stream: {e}"),
            CaptureError::Play(e) => write!(f, "failed to start input stream: {e}"),
            CaptureError::Pause(e) => write!(f, "failed to pause input stream: {e}"),
        }
    }
}

impl Error for CaptureError {}

impl From<cpal::DeviceNameError> for CaptureError {
    fn from(e: cpal::DeviceNameError) -> Self {
        CaptureError::DeviceName(e.to_string())
    }
}

impl From<cpal::DevicesError> for CaptureError {
    fn from(e: cpal::DevicesError) -> Self {
        CaptureError::DeviceName(e.to_string())
    }
}

impl From<cpal::DefaultStreamConfigError> for CaptureError {
    fn from(e: cpal::DefaultStreamConfigError) -> Self {
        CaptureError::StreamConfig(e.to_string())
    }
}

impl From<cpal::BuildStreamError> for CaptureError {
    fn from(e: cpal::BuildStreamError) -> Self {
        CaptureError::BuildStream(e.to_string())
    }
}

impl From<cpal::PlayStreamError> for CaptureError {
    fn from(e: cpal::PlayStreamError) -> Self {
        CaptureError::Play(e.to_string())
    }
}

impl From<cpal::PauseStreamError> for CaptureError {
    fn from(e: cpal::PauseStreamError) -> Self {
        CaptureError::Pause(e.to_string())
    }
}

/// A live microphone capture. Drops (or [`pause`](MicCapture::pause)) to stop.
pub struct MicCapture {
    stream: cpal::Stream,
    device_name: String,
    sample_rate: u32,
    channels: u16,
}

impl MicCapture {
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Start the stream. `open_default`/`open_named` build but do not play.
    pub fn play(&self) -> Result<(), CaptureError> {
        self.stream.play()?;
        Ok(())
    }

    pub fn pause(&self) -> Result<(), CaptureError> {
        self.stream.pause()?;
        Ok(())
    }
}

/// Names of all available input (microphone) devices.
pub fn list_input_devices() -> Result<Vec<String>, CaptureError> {
    let host = cpal::default_host();
    let mut names = Vec::new();
    for device in host.input_devices()? {
        names.push(device.name().unwrap_or_else(|_| "<unnamed>".to_string()));
    }
    Ok(names)
}

/// Name of the system-default input device, if any.
pub fn default_input_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .and_then(|d| d.name().ok())
}

/// Open the default input device and stream captured frames to `tx`.
pub fn open_default(tx: Sender<Frame>) -> Result<MicCapture, CaptureError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(CaptureError::NoDevice)?;
    open_device(device, tx)
}

/// Open a named input device and stream captured frames to `tx`.
pub fn open_named(name: &str, tx: Sender<Frame>) -> Result<MicCapture, CaptureError> {
    let host = cpal::default_host();
    let device = host
        .input_devices()?
        .find(|d| d.name().map(|n| n == name).unwrap_or(false))
        .ok_or_else(|| CaptureError::DeviceNotFound(name.to_string()))?;
    open_device(device, tx)
}

fn open_device(device: cpal::Device, tx: Sender<Frame>) -> Result<MicCapture, CaptureError> {
    let device_name = device.name().unwrap_or_else(|_| "<unnamed>".to_string());
    let cfg = device.default_input_config()?;
    let sample_rate = cfg.sample_rate().0;
    let channels = cfg.channels();
    let stream_cfg: cpal::StreamConfig = cfg.clone().into();

    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => {
            build_stream::<f32>(&device, &stream_cfg, channels, sample_rate, tx.clone())?
        }
        cpal::SampleFormat::I16 => {
            build_stream::<i16>(&device, &stream_cfg, channels, sample_rate, tx.clone())?
        }
        cpal::SampleFormat::U16 => {
            build_stream::<u16>(&device, &stream_cfg, channels, sample_rate, tx.clone())?
        }
        cpal::SampleFormat::I32 => {
            build_stream::<i32>(&device, &stream_cfg, channels, sample_rate, tx)?
        }
        cpal::SampleFormat::F64 => {
            build_stream::<f64>(&device, &stream_cfg, channels, sample_rate, tx)?
        }
        other => {
            return Err(CaptureError::StreamConfig(format!(
                "unsupported sample format: {other:?}"
            )))
        }
    };

    Ok(MicCapture {
        stream,
        device_name,
        sample_rate,
        channels,
    })
}

/// Build an input stream for a specific sample type `T`, converting each
/// callback buffer to mono `f32` and sending a [`Frame`] down the channel.
/// Never blocks: only downmix + RMS + `try_send` happen on the audio thread.
fn build_stream<T>(
    device: &cpal::Device,
    stream_cfg: &cpal::StreamConfig,
    channels: u16,
    sample_rate: u32,
    tx: Sender<Frame>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let channels = channels as usize;
    let channels_u16 = channels as u16;
    let error_fn = |e| eprintln!("[mic] stream error: {e}");

    let data_fn = move |data: &[T], _: &cpal::InputCallbackInfo| {
        let samples: Vec<f32> = data.iter().map(|&s| s.to_sample::<f32>()).collect();
        let samples = super::processing::downmix_to_mono(&samples, channels);
        let rms = compute_rms(&samples);
        let _ = tx.send(Frame {
            samples,
            sample_rate,
            channels: channels_u16,
            rms,
        });
    };

    device.build_input_stream(stream_cfg, data_fn, error_fn, None)
}

/// Root-mean-square amplitude of a mono buffer.
fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_known_buffer() {
        // RMS of [1.0; 4] is 1.0
        assert!((compute_rms(&[1.0, 1.0, 1.0, 1.0]) - 1.0).abs() < 1e-6);
        // RMS of [0.0; 4] is 0.0
        assert_eq!(compute_rms(&[0.0, 0.0, 0.0, 0.0]), 0.0);
        // empty buffer -> 0
        assert_eq!(compute_rms(&[]), 0.0);
    }
}
