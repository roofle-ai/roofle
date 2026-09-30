//! OpenAI-compatible AI plumbing: settings, API key resolution, HTTP client.

mod client;
mod config;
mod key;
mod notes_client;

pub use client::{AiClient, ChatMessage};
pub use config::{data_dir, load_for, save, AiConfig, AiConfigView, ProviderKind};
pub use key::{env_api_key, resolve_api_key};
pub use notes_client::NotesClient;
