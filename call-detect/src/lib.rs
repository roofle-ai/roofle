//! Notices when a Zoom or Microsoft Teams call starts on this device.
//!
//! Layers, innermost first:
//! - [`detect`]: pure decision from a [`Snapshot`] of the machine.
//! - [`CallDetector`]: turns repeated decisions into one "call started" event.
//! - [`probe`]: reads the real machine into a [`Snapshot`].
//! - [`watch`]: polls the probe on a background thread.

mod app;
mod detector;
mod mic;
mod probe;
mod watcher;

pub use app::{detect, CallApp, Snapshot};
pub use detector::CallDetector;
pub use watcher::watch;
