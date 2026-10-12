//! AI note generation for spoken sessions — a two-agent flow.
//!
//! The note logic lives here (meeting intelligence), independent of HTTP,
//! keychain and UI. The app shell supplies a [`CompletionClient`] — the actual
//! LLM call — so this crate stays pure and testable.
//!
//!   1. **Writer agent**: given the latest transcript window + the notes produced so
//!      far, produce the updated notes (merging new info, editing/restructuring
//!      what already exists).
//!   2. **Validator agent**: given the transcript window + the writer's
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

    /// Like [`complete`](Self::complete), but the reply must match the JSON
    /// `schema` (temperature 0, strict). Defaults to plain `complete` so
    /// clients without structured output keep working.
    fn complete_json(
        &self,
        system: &str,
        user: &str,
        _schema: &serde_json::Value,
    ) -> Result<String, String> {
        self.complete(system, user)
    }
}

/// Steers the writer toward concise, evolving session notes.
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

/// Run the writer then validator over one notes window.
///
/// `transcript` is everything said since the last update; `existing_notes` is
/// the notes document produced so far (empty on the first window); `goal` is
/// the call's stated purpose (may be empty); `ended` marks the final pass.
/// These are the writer prompt's four inputs.
pub fn agent_notes(
    client: &dyn CompletionClient,
    transcript: &str,
    existing_notes: &str,
    goal: &str,
    ended: bool,
    log: bool,
) -> Result<String, String> {
    log_step(
        log,
        "writer",
        &format!("window={} chars, existing_notes={} chars", transcript.chars().count(), existing_notes.chars().count()),
    );

    let mut writer_user = format!(
        "NOTES SO FAR:\n{}\n\nLATEST WINDOW:\n{transcript}",
        if existing_notes.trim().is_empty() { "(none yet)" } else { existing_notes },
    );
    if !goal.trim().is_empty() {
        writer_user.push_str(&format!("\n\nGOAL:\n{}", goal.trim()));
    }
    if ended {
        writer_user.push_str("\n\nSESSION ENDED: true");
    }
    let written = client.complete(WRITER_SYSTEM, &writer_user)?;
    log_step(log, "writer", &format!("returned {} chars", written.chars().count()));

    log_step(log, "validator", "checking writer notes against the window");
    let validator_user = format!("Transcript window:\n{transcript}\n\nDraft notes to validate:\n{written}");
    let validated = client.complete(VALIDATOR_SYSTEM, &validator_user)?;
    log_step(log, "validator", &format!("returned {} chars", validated.chars().count()));

    Ok(validated.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Replies per agent; records which agents ran and what each was given.
    struct Script {
        calls: RefCell<Vec<(&'static str, String)>>,
    }
    impl CompletionClient for Script {
        fn complete(&self, system: &str, user: &str) -> Result<String, String> {
            let (role, reply) = if system.contains("You are a note-taker") {
                ("writer", "writer saw input")
            } else {
                ("validator", "validator saw input")
            };
            self.calls.borrow_mut().push((role, user.to_string()));
            Ok(reply.to_string())
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
        let client = Script { calls: RefCell::new(Vec::new()) };
        let notes = agent_notes(&client, "window", "old notes", "ship v2", true, false).unwrap();
        assert!(notes.contains("validator saw"), "final notes must come from validator: {notes}");
        let calls = client.calls.borrow();
        let order: Vec<_> = calls.iter().map(|(r, _)| *r).collect();
        assert_eq!(order, ["writer", "validator"]);
            }

    #[test]
    fn writer_gets_every_input_its_prompt_names() {
        let client = Script { calls: RefCell::new(Vec::new()) };
        agent_notes(&client, "window text", "old notes", "ship v2", true, false).unwrap();
        let user = client.calls.borrow()[0].1.clone();
        for part in ["NOTES SO FAR:\nold notes", "LATEST WINDOW:\nwindow text", "GOAL:\nship v2", "SESSION ENDED: true"] {
            assert!(user.contains(part), "missing {part:?} in {user}");
        }
    }

    #[test]
    fn optional_inputs_are_omitted_when_absent() {
        let client = Script { calls: RefCell::new(Vec::new()) };
        agent_notes(&client, "first", "", "", false, false).unwrap();
        let user = client.calls.borrow()[0].1.clone();
        assert!(user.contains("(none yet)"));
        assert!(!user.contains("GOAL:") && !user.contains("SESSION ENDED"));
    }

    #[test]
    fn prompts_are_embedded_and_agree_on_sections() {
        for section in ["## Summary", "## Decisions", "## Action Items"] {
            assert!(WRITER_SYSTEM.contains(section), "writer missing {section}");
        }
        for section in ["Decisions", "Action Items"] {
            assert!(VALIDATOR_SYSTEM.contains(section), "validator missing {section}");
        }
    }
}
