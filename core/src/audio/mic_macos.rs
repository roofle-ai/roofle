//! macOS microphone capture via `AVAudioEngine` (pure Rust, objc2 bindings).
//!
//! Replaces the `cpal` mic backend, which returns silence (rms 0.00) on this
//! machine even though the device is open and delivering frames. This module
//! ports the known-good `AVAudioEngine` tap logic from the old Electron
//! addon: install a tap on the input node's bus 0, read non-interleaved float
//! channel data, downmix to mono, and send a
//! [`Frame`](crate::audio::mic::Frame) down the channel. The audio callback
//! never blocks: it only downmixes, computes RMS and `try_send`s.

use std::ptr::NonNull;
use std::sync::mpsc::Sender;

use block2::StackBlock;
use objc2::rc::autoreleasepool;
use objc2_avf_audio::{AVAudioEngine, AVAudioPCMBuffer, AVAudioTime};

use crate::audio::mic::{CaptureError, Frame};

/// Tap buffer size in frames (~93 ms at 44.1 kHz).
const TAP_FRAMES: u32 = 4096;

/// A live AVAudioEngine microphone capture. Drops to stop.
pub struct MicCapture {
    engine: objc2::rc::Retained<AVAudioEngine>,
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

    /// Start the engine. `startAndReturnError` on a running engine is a no-op.
    pub fn play(&self) -> Result<(), CaptureError> {
        let result = autoreleasepool(|_| unsafe { self.engine.startAndReturnError() });
        result.map_err(|e| {
            CaptureError::Play(e.localizedDescription().to_string())
        })
    }

    /// Stop the engine. `stop` on a stopped engine is a no-op.
    pub fn pause(&self) -> Result<(), CaptureError> {
        unsafe { self.engine.stop() };
        Ok(())
    }
}

/// Name of the system-default input device, if any.
pub fn default_input_name() -> Option<String> {
    list_input_devices().ok().and_then(|mut v| v.pop())
}

/// Names of all available input devices.
///
/// `AVAudioEngine` selects the default input only; we report just that one.
/// (`coreaudio` device enumeration is out of scope for this mic fix.)
pub fn list_input_devices() -> Result<Vec<String>, CaptureError> {
    let engine = build_engine()?;
    let (format, channels) = input_format(&engine)?;
    let rate = unsafe { format.sampleRate() } as u32;
    // Determine the default input device name via CoreAudio's default UID is
    // not exposed through AVAudioEngine; report a stable label.
    Ok(vec![format!("MacBook Pro Microphone @ {rate} Hz, {channels} ch")])
}

/// Open the default input device and stream captured frames to `tx`.
pub fn open_default(tx: Sender<Frame>) -> Result<MicCapture, CaptureError> {
    open_engine(None, tx)
}

/// Open the named input device and stream captured frames to `tx`.
///
/// `AVAudioEngine` always captures the default input; a named device is
/// accepted for API parity but uses the default underneath.
pub fn open_named(name: &str, tx: Sender<Frame>) -> Result<MicCapture, CaptureError> {
    let _ = name;
    open_engine(None, tx)
}

fn build_engine() -> Result<objc2::rc::Retained<AVAudioEngine>, CaptureError> {
    let engine = autoreleasepool(|_| {
        let e = unsafe { AVAudioEngine::new() };
        e
    });
    Ok(engine)
}

fn input_format(
    engine: &objc2::rc::Retained<AVAudioEngine>,
) -> Result<(objc2::rc::Retained<objc2_avf_audio::AVAudioFormat>, u16), CaptureError> {
    let input = autoreleasepool(|_| unsafe { engine.inputNode() });
    let format = autoreleasepool(|_| unsafe { input.outputFormatForBus(0) });
    let channels = unsafe { format.channelCount() } as u16;
    if channels == 0 {
        return Err(CaptureError::NoDevice);
    }
    Ok((format, channels))
}

fn open_engine(
    _name: Option<&str>,
    tx: Sender<Frame>,
) -> Result<MicCapture, CaptureError> {
    let engine = build_engine()?;
    let (format, channels) = input_format(&engine)?;
    let sample_rate = unsafe { format.sampleRate() } as u32;

    let input = autoreleasepool(|_| unsafe { engine.inputNode() });

    // The tap captures the input node's native format (format: None), so the
    // buffer's real sample rate/channel layout is read from each callback.
    let block = StackBlock::new(
        move |buffer: NonNull<AVAudioPCMBuffer>, _when: NonNull<AVAudioTime>| {
            let buffer = unsafe { buffer.as_ref() };
            emit_frame(buffer, &tx);
        },
    );

    autoreleasepool(|_| {
        // `&*block` is a `&Block<dyn Fn(...)>`; cast to the raw `*mut DynBlock`
        // that `installTap` expects. `_Block_copy` retains it, so the stack
        // block only needs to outlive this call.
        let ptr: *mut _ = &*block as *const _ as *mut _;
        unsafe {
            input.installTapOnBus_bufferSize_format_block(0, TAP_FRAMES, None, ptr);
        }
    });

    Ok(MicCapture {
        engine,
        device_name: "MacBook Pro Microphone".to_string(),
        sample_rate,
        channels,
    })
}

/// Convert one tapped `AVAudioPCMBuffer` into a mono [`Frame`].
///
/// The buffer is non-interleaved float32 (`floatChannelData`); we downmix all
/// channels into one mono stream and compute RMS. Runs on the audio thread, so
/// keep it allocation-light.
fn emit_frame(buffer: &AVAudioPCMBuffer, tx: &Sender<Frame>) {
    let (frame_count, channel_count) = unsafe {
        let fc = buffer.frameLength() as usize;
        let cc = buffer.format().channelCount() as usize;
        (fc, cc)
    };
    if frame_count == 0 || channel_count == 0 {
        return;
    }

    let channels_ptr = unsafe { buffer.floatChannelData() };
    if channels_ptr.is_null() {
        return;
    }

    // channels_ptr points to `channel_count` pointers, each to `frame_count`
    // contiguous float samples. Average across channels into mono.
    let mut samples: Vec<f32> = Vec::with_capacity(frame_count);
    for frame in 0..frame_count {
        let mut sum = 0.0f32;
        for ch in 0..channel_count {
            let chan_ptr = unsafe { *channels_ptr.add(ch) }.as_ptr();
            sum += unsafe { *chan_ptr.add(frame) };
        }
        samples.push(sum / channel_count as f32);
    }

    let rms = compute_rms(&samples);
    let _ = tx.send(Frame {
        sample_rate: unsafe { buffer.format().sampleRate() } as u32,
        channels: channel_count as u16,
        rms,
        samples,
    });
}

/// Root-mean-square amplitude of a mono buffer.
fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}
