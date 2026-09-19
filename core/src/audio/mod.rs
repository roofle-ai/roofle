//! Audio capture & processing.
//!
//! - `mic`          — microphone capture via `cpal` (S3)
//! - `processing`   — normalize to 16 kHz mono: downmix + rubato resample + gain (S4)
//! - `system_macos` — system-audio output capture via ScreenCaptureKit (S11, macOS only)

pub mod mic;
pub mod processing;

#[cfg(target_os = "macos")]
pub mod mic_macos;
#[cfg(target_os = "macos")]
pub mod system_macos;
