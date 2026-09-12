//! Audio capture & processing.
//!
//! - `mic`      — microphone capture via `cpal` (S3)
//! - `processing` — resample to 16 kHz mono + downmix + gain (S4, not yet)
//!
//! Platform-specific system-audio capture lands later: `system_macos`
//! (screencapturekit, S11) and `system_windows` (WASAPI loopback, S12).

pub mod mic;
