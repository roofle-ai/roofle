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

/// Generic writer prompt, used when the call type has no file of its own.
/// Loaded from `prompts/notes/writer.txt` at compile time.
pub const WRITER_SYSTEM: &str = include_str!("../prompts/notes/writer.txt");

/// Rules shared by every call type (inputs, limits, accuracy, style).
/// Loaded from `prompts/notes/base.txt` at compile time.
const WRITER_BASE: &str = include_str!("../prompts/notes/base.txt");

/// Call-type part of the writer prompt, keyed by the value of the call-setup
/// dropdown (`src/ui/partials/transcript.html`, `#call-setup-type`). To add one:
/// drop `prompts/notes/call-types/<value>.txt` and add a line here. Values not
/// listed use [`WRITER_SYSTEM`].
fn call_type_prompt(call_type: &str) -> Option<&'static str> {
    Some(match call_type {
        "sales-selling" => include_str!("../prompts/notes/call-types/sales-selling.txt"),
        "sales-buying" => include_str!("../prompts/notes/call-types/sales-buying.txt"),
        "taking-interview" => include_str!("../prompts/notes/call-types/taking-interview.txt"),
        "giving-interview" => include_str!("../prompts/notes/call-types/giving-interview.txt"),
        "discovery" => include_str!("../prompts/notes/call-types/discovery.txt"),
        "decision" => include_str!("../prompts/notes/call-types/decision.txt"),
        "status" => include_str!("../prompts/notes/call-types/status.txt"),
        "planning" => include_str!("../prompts/notes/call-types/planning.txt"),
        "review" => include_str!("../prompts/notes/call-types/review.txt"),
        "negotiation" => include_str!("../prompts/notes/call-types/negotiation.txt"),
        "one-on-one" => include_str!("../prompts/notes/call-types/one-on-one.txt"),
        "training" => include_str!("../prompts/notes/call-types/training.txt"),
        _ => return None,
    })
}

/// The writer's system prompt: base rules + call-type file, or the generic prompt.
pub fn writer_system(call_type: &str) -> String {
    match call_type_prompt(call_type) {
        Some(call_type) => format!("{WRITER_BASE}\n{call_type}"),
        None => WRITER_SYSTEM.to_string(),
    }
}

/// Steers the validator toward checking the writer's notes against the source.
/// Loaded from `prompts/notes/validator.txt` at compile time.
pub const VALIDATOR_SYSTEM: &str = include_str!("../prompts/notes/validator.txt");

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
/// the call's stated purpose (may be empty); `persona` is who the user is
/// (from `PersonaProfile::describe`, may be empty); `call_type` is the value picked
/// in the call dropdown and selects the writer prompt (see [`writer_system`], may
/// be empty); `ended` marks the final pass.
pub fn agent_notes(
    client: &dyn CompletionClient,
    transcript: &str,
    existing_notes: &str,
    goal: &str,
    persona: &str,
    call_type: &str,
    ended: bool,
    log: bool,
) -> Result<String, String> {
    log_step(
        log,
        "writer",
        &format!("window={} chars, existing_notes={} chars", transcript.chars().count(), existing_notes.chars().count()),
    );

    let mut context = String::new();
    if !goal.trim().is_empty() {
        context.push_str(&format!("\n\nGOAL:\n{}", goal.trim()));
    }
    if !persona.trim().is_empty() {
        context.push_str(&format!("\n\nABOUT THE USER:\n{}", persona.trim()));
    }
    let prior = if existing_notes.trim().is_empty() { "(none yet)" } else { existing_notes };
    let mut writer_user = format!("NOTES SO FAR:\n{prior}\n\nLATEST WINDOW:\n{transcript}{context}");
    if ended {
        writer_user.push_str("\n\nSESSION ENDED: true");
    }
    let written = client.complete(&writer_system(call_type), &writer_user)?;
    log_step(log, "writer", &format!("returned {} chars", written.chars().count()));

    log_step(log, "validator", "checking writer notes against the window");
    let validator_user =
        format!("NOTES BEFORE THIS WINDOW:\n{prior}\n\nLATEST WINDOW:\n{transcript}{context}\n\nDRAFT:\n{written}");
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
        let notes = agent_notes(&client, "window", "old notes", "ship v2", "I am a PM.", "", true, false).unwrap();
        assert!(notes.contains("validator saw"), "final notes must come from validator: {notes}");
        let calls = client.calls.borrow();
        let order: Vec<_> = calls.iter().map(|(r, _)| *r).collect();
        assert_eq!(order, ["writer", "validator"]);
        let validator_user = &calls[1].1;
        for part in ["NOTES BEFORE THIS WINDOW:\nold notes", "GOAL:\nship v2", "ABOUT THE USER:\nI am a PM."] {
            assert!(validator_user.contains(part), "validator missing {part:?}: {validator_user}");
        }
    }

    #[test]
    fn writer_gets_every_input_its_prompt_names() {
        let client = Script { calls: RefCell::new(Vec::new()) };
        agent_notes(&client, "window text", "old notes", "ship v2", "I am a PM.", "", true, false).unwrap();
        let user = client.calls.borrow()[0].1.clone();
        for part in ["NOTES SO FAR:\nold notes", "LATEST WINDOW:\nwindow text", "GOAL:\nship v2", "ABOUT THE USER:\nI am a PM.", "SESSION ENDED: true"] {
            assert!(user.contains(part), "missing {part:?} in {user}");
        }
    }

    #[test]
    fn optional_inputs_are_omitted_when_absent() {
        let client = Script { calls: RefCell::new(Vec::new()) };
        agent_notes(&client, "first", "", "", "", "", false, false).unwrap();
        let user = client.calls.borrow()[0].1.clone();
        assert!(user.contains("(none yet)"));
        assert!(!user.contains("GOAL:") && !user.contains("ABOUT THE USER") && !user.contains("SESSION ENDED"));
    }

    #[test]
    fn call_type_with_a_file_gets_base_plus_its_prompt() {
        for id in [
            "sales-selling", "sales-buying", "taking-interview", "giving-interview", "discovery", "decision",
            "status", "planning", "review", "negotiation", "one-on-one", "training",
        ] {
            let prompt = writer_system(id);
            assert!(prompt.starts_with(WRITER_BASE), "{id} must start with the base prompt");
            assert!(prompt.contains("## CALL TYPE:"), "{id} missing its heading");
            for part in ["## Summary", "## SECTIONS", "## LENGTH LIMITS"] {
                assert!(prompt.contains(part), "{id} writer missing {part}");
            }
        }
    }

    #[test]
    fn unknown_or_empty_call_type_falls_back_to_generic_prompt() {
        for id in ["", "other", "no-such-type"] {
            assert_eq!(writer_system(id), WRITER_SYSTEM);
        }
    }

    #[test]
    fn prompts_are_embedded_and_agree_on_sections() {
        for section in ["## Summary", "## Decisions", "## Action Items"] {
            assert!(WRITER_SYSTEM.contains(section), "writer missing {section}");
        }
    }
}
