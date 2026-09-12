//! Audio capture & processing.
//!
//! - `mic`          — microphone capture via `cpal` (S3)
//! - `processing`   — normalize to 16 kHz mono: downmix + rubato resample + gain (S4)
//!
//! Platform-specific system-audio capture lands later: `system_macos`
//! (screencapturekit, S11) and `system_windows` (WASAPI loopback, S12).

pub mod mic;
pub mod processing;
