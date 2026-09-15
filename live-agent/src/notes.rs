//! AI meeting-note generation — a two-agent flow.
//!
//! The note logic lives here (meeting intelligence), independent of HTTP,
//! keychain and UI. The app shell supplies a [`CompletionClient`] — the actual
//! LLM call — so this crate stays pure and testable.
//!
//! Flow per paragraph boundary:
//!   1. **Writer agent**: given the latest paragraph + the notes produced so
//!      far, produce the updated notes (merging new info, editing/restructuring
//!      what already exists).
//!   2. **Validator agent**: given the transcript paragraph + the writer's
//!      notes, check for inaccuracies / missed items and return the corrected
//!      final notes.
//!
//! Debug logging is gated behind the `ROOFLE_LOG_AI` env var (`1`/`true`/`on`),
//! so the agent's every step is observable during development and silent in
//! production.

use std::sync::OnceLock;

/// A minimal LLM completion capability, implemented by the app shell.
pub trait CompletionClient {
    /// Complete a chat: system prompt + user message → assistant reply text.
    fn complete(&self, system: &str, user: &str) -> Result<String, String>;
}

/// Steers the writer toward concise, evolving meeting notes.
/// Loaded from `prompts/writer.txt` at compile time.
pub const WRITER_SYSTEM: &str = include_str!("../prompts/writer.txt");

/// Steers the validator toward checking the writer's notes against the source.
/// Loaded from `prompts/validator.txt` at compile time.
pub const VALIDATOR_SYSTEM: &str = include_str!("../prompts/validator.txt");

/// Env var enabling AI-agent debug logging (`1`, `true` or `on`).
pub const LOG_ENV_VAR: &str = "ROOFLE_LOG_AI";

static LOG_ENABLED: OnceLock<bool> = OnceLock::new();

/// Whether AI-agent debug logging is on, per the `ROOFLE_LOG_AI` env var.
pub fn logging_enabled() -> bool {
    *LOG_ENABLED.get_or_init(|| {
        std::env::var(LOG_ENV_VAR)
            .map(|v| {
                let v = v.trim().to_ascii_lowercase();
                v == "1" || v == "true" || v == "on"
            })
            .unwrap_or(false)
    })
}

/// Print an agent step to stderr when logging is enabled.
pub fn log_step(enabled: bool, step: &str, msg: &str) {
    if enabled {
        eprintln!("[roofle-ai] {step}: {msg}");
    }
}

/// Run the two-agent note update for one paragraph boundary.
///
/// `paragraph` is the newly accumulated transcript chunk; `existing_notes` is
/// the notes document produced so far (empty on the first paragraph). Returns
/// the validator's final notes document.
pub fn agent_notes(
    client: &dyn CompletionClient,
    paragraph: &str,
    existing_notes: &str,
    log: bool,
) -> Result<String, String> {
    let input_chars = paragraph.chars().count();
    log_step(
        log,
        "writer",
        &format!("paragraph={input_chars} chars, existing_notes={} chars", existing_notes.chars().count()),
    );

    let writer_user = if existing_notes.trim().is_empty() {
        format!("Latest transcript paragraph:\n{paragraph}")
    } else {
        format!(
            "Notes so far:\n{existing_notes}\n\nLatest transcript paragraph:\n{paragraph}"
        )
    };
    let written = client.complete(WRITER_SYSTEM, &writer_user)?;
    log_step(log, "writer", &format!("returned {} chars", written.chars().count()));

    log_step(log, "validator", "checking writer notes against paragraph");
    let validator_user = format!(
        "Transcript paragraph:\n{paragraph}\n\nDraft notes to validate:\n{written}"
    );
    let validated = client.complete(VALIDATOR_SYSTEM, &validator_user)?;
    log_step(log, "validator", &format!("returned {} chars", validated.chars().count()));

    Ok(validated.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A client that echoes the last user message with a marker, so tests can
    /// observe what each agent was given.
    struct Echo;
    impl CompletionClient for Echo {
        fn complete(&self, system: &str, user: &str) -> Result<String, String> {
            let role = if system.contains("note-taker") {
                "writer"
            } else {
                "validator"
            };
            let _ = user;
            Ok(format!("{role} saw input"))
        }
    }

    #[test]
    fn logging_defaults_off() {
        // Unset env in tests → logging disabled unless explicitly enabled.
        unsafe { std::env::remove_var(LOG_ENV_VAR) };
        assert!(!logging_enabled());
    }

    #[test]
    fn agent_runs_writer_then_validator() {
        let out = agent_notes(&Echo, "new para", "old notes", false).unwrap();
        assert!(out.contains("validator saw"), "got: {out}");
        assert!(!out.contains("writer saw"), "final notes must come from validator");
    }

    #[test]
    fn agent_passes_empty_notes_on_first_run() {
        let out = agent_notes(&Echo, "first para", "", false).unwrap();
        assert!(out.contains("validator saw"), "got: {out}");
    }
}
