//! Background LLM agents triggered by the recording engine.

mod client;
pub use client::{stage_clients, StageClients};
pub mod notes;
pub mod title;
pub mod wrap_up;
