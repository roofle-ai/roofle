//! # roofle-core
//!
//! Pure-Rust shared library for the Roofle real-time transcription app.
//! Deliberately free of any Tauri dependency so the audio -> ASR pipeline can
//! be validated headlessly (via `roofle-cli`) and unit-tested in isolation.
//!
//! ## Module layout (filled in slice by slice, see plans/implementation-plan.md)
//!
//! - `audio`      — mic capture (cpal), macOS/Windows system audio, processing
//! - `asr`        — sherpa-onnx streaming engine + built-in VAD, segment types
//! - `pipeline`   — ring buffer -> VAD -> streaming ASR -> typed events
//!
//! ## Slice status
//!
//! - S1: workspace scaffold
//! - S2: CI matrix
//! - S3: `audio::mic` — cpal capture with RMS levels
//! - S4: `audio::processing` — downmix + rubato resample to 16 kHz + gain
//! - S5: `asr` — load streaming Zipformer-Transducer model, transcribe a known
//!   test WAV (official `sherpa-onnx` bindings)
//! - S6: `asr::StreamingSession` — live 16 kHz chunks in, partial + final
//!   segments out, driven by `roofle-cli stream`
//! - S7 (current): `pipeline` — threaded pipeline: capture thread pushes 16 kHz
//!   mono into a ring buffer, a background ASR thread drains it and emits typed
//!   partial/final `Segment` events; `Source` trait (mic shipped, system audio
//!   later). `roofle-cli stream` now runs through the pipeline API.

pub mod asr;
pub mod audio;
pub mod pipeline;

/// Version of the shared library, taken from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
