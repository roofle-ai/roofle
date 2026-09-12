//! # roofle-core
//!
//! Pure-Rust shared library for the Roofle real-time transcription app.
//! Deliberately free of any Tauri dependency so the audio -> ASR pipeline can
//! be validated headlessly (via `roofle-cli`) and unit-tested in isolation.
//!
//! ## Module layout (filled in slice by slice, see plans/implementation-plan.md)
//!
//! - `audio`      — mic capture (cpal), macOS/Windows system audio, processing
//! - `asr`        — sherpa-rs streaming engine + built-in VAD, segment types
//! - `pipeline`   — ring buffer -> VAD -> streaming ASR -> typed events
//!
//! ## Slice status
//!
//! - S1: workspace scaffold
//! - S2: CI matrix
//! - S3 (current): `audio::mic` — cpal capture with RMS levels

pub mod audio;

/// Version of the shared library, taken from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
