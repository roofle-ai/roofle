//! Names of the events emitted to the frontend.

pub const TRANSCRIPT: &str = "transcript-update";
pub const LEVEL: &str = "level";
pub const NOTES: &str = "notes-update";
pub const CHALLENGES: &str = "challenge-update";
pub const RECORDING_STATE: &str = "recording-state";
pub const APP_STATUS: &str = "app-status";
pub const TITLE: &str = "title-update";
/// The end-of-call notes pass finished; recipes can now run on the final notes.
pub const WRAP_UP: &str = "call-wrapped-up";
/// The tray asked to start a call; the UI collects the goal first.
pub const START_REQUESTED: &str = "start-requested";
