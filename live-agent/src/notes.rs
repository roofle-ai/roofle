//! AI note generation for spoken sessions — a three-agent flow.
//!
//! The note logic lives here (meeting intelligence), independent of HTTP,
//! keychain and UI. The app shell supplies a [`CompletionClient`] — the actual
//! LLM call — so this crate stays pure and testable.
//!
//! Flow per paragraph boundary:
//!   0. **Classifier agent**: gauge whether the session is a meeting, a
//!      general conversation or a presentation, so the notes use the right
//!      shape (a chat has no "decisions"). It runs until its verdict is
//!      stable, then only re-checks occasionally.
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

use serde::Deserialize;

/// A minimal LLM completion capability, implemented by the app shell.
pub trait CompletionClient {
    /// Complete a chat: system prompt + user message → assistant reply text.
    fn complete(&self, system: &str, user: &str) -> Result<String, String>;
}

/// Steers the classifier toward telling meetings, conversations and
/// presentations apart. Loaded from `prompts/classifier.txt` at compile time.
pub const CLASSIFIER_SYSTEM: &str = include_str!("../prompts/classifier.txt");

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

/// What kind of spoken session this is; decides the shape of the notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConversationKind {
    /// People working toward an outcome: decisions, tasks, status.
    Meeting,
    /// Talk without a working goal. The safe default: it never invents
    /// decisions or assignments.
    #[default]
    Conversation,
    /// Mostly one speaker addressing listeners.
    Presentation,
}

impl ConversationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Meeting => "meeting",
            Self::Conversation => "conversation",
            Self::Presentation => "presentation",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "meeting" => Some(Self::Meeting),
            "conversation" => Some(Self::Conversation),
            "presentation" => Some(Self::Presentation),
            _ => None,
        }
    }
}

/// How sure the classifier was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "high" => Self::High,
            "medium" => Self::Medium,
            _ => Self::Low,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Verdict {
    kind: ConversationKind,
    confidence: Confidence,
}

/// Consecutive confident, agreeing verdicts after which the kind is settled
/// and the classifier call is skipped.
const SETTLE_AFTER: u8 = 3;
/// Once settled, re-check every this many paragraphs, since a session can
/// change character (small talk, then a real meeting).
const RECHECK_EVERY: u8 = 6;

/// The session's current kind plus how settled it is. The app shell keeps one
/// per live session and passes it back in on every paragraph.
#[derive(Debug, Clone, Copy, Default)]
pub struct KindState {
    pub kind: ConversationKind,
    /// Consecutive non-low verdicts agreeing with `kind`.
    agreeing: u8,
    /// Paragraphs since the classifier last ran (counted while settled).
    since_check: u8,
}

impl KindState {
    fn settled(&self) -> bool {
        self.agreeing >= SETTLE_AFTER
    }

    /// Whether the classifier should run for this paragraph.
    fn needs_check(&self) -> bool {
        !self.settled() || self.since_check >= RECHECK_EVERY
    }

    /// Fold one classifier verdict in. A low-confidence verdict never moves an
    /// established kind; a settled kind only yields to a high-confidence one.
    fn apply(self, v: Verdict, had_prior: bool) -> Self {
        if !had_prior {
            return Self {
                kind: v.kind,
                agreeing: u8::from(v.confidence != Confidence::Low),
                since_check: 0,
            };
        }
        if v.kind == self.kind {
            let agreeing = if v.confidence == Confidence::Low {
                self.agreeing
            } else {
                self.agreeing.saturating_add(1)
            };
            return Self { agreeing, since_check: 0, ..self };
        }
        let may_switch = match v.confidence {
            Confidence::Low => false,
            Confidence::Medium => !self.settled(),
            Confidence::High => true,
        };
        if may_switch {
            Self { kind: v.kind, agreeing: 1, since_check: 0 }
        } else {
            Self { since_check: 0, ..self }
        }
    }
}

#[derive(Deserialize)]
struct RawVerdict {
    kind: String,
    #[serde(default)]
    confidence: String,
}

/// Parse the classifier reply, tolerating prose or a code fence around it.
fn parse_verdict(raw: &str) -> Option<Verdict> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')? + 1;
    let parsed: RawVerdict = serde_json::from_str(raw.get(start..end)?).ok()?;
    Some(Verdict {
        kind: ConversationKind::parse(&parsed.kind)?,
        confidence: Confidence::parse(&parsed.confidence),
    })
}

/// Classifier step. Never fails the notes: if the call or the reply is bad,
/// the prior kind (or the safe default) carries on.
fn classify(
    client: &dyn CompletionClient,
    paragraph: &str,
    existing_notes: &str,
    prior: Option<KindState>,
    log: bool,
) -> KindState {
    let current = prior.unwrap_or_default();
    if prior.is_some() && !current.needs_check() {
        return KindState { since_check: current.since_check.saturating_add(1), ..current };
    }

    let notes = if existing_notes.trim().is_empty() { "(none yet)" } else { existing_notes };
    let user = format!(
        "Kind so far: {}\n\nNotes so far:\n{notes}\n\nLatest transcript paragraph:\n{paragraph}",
        if prior.is_some() { current.kind.as_str() } else { "(not decided yet)" },
    );
    match client.complete(CLASSIFIER_SYSTEM, &user).ok().as_deref().and_then(parse_verdict) {
        Some(v) => {
            let next = current.apply(v, prior.is_some());
            log_step(
                log,
                "classifier",
                &format!("{:?} -> {} (agreeing={})", v, next.kind.as_str(), next.agreeing),
            );
            next
        }
        None => {
            log_step(log, "classifier", "no usable verdict; keeping previous kind");
            current
        }
    }
}

/// Result of one note update.
pub struct NotesOutcome {
    /// The validator's final notes document.
    pub notes: String,
    /// Classifier state to pass back in with the next paragraph.
    pub kind: KindState,
}

/// Run the three-agent note update for one paragraph boundary.
///
/// `paragraph` is the newly accumulated transcript chunk; `existing_notes` is
/// the notes document produced so far (empty on the first paragraph); `prior`
/// is the [`KindState`] returned by the previous call for this session (`None`
/// on the first).
pub fn agent_notes(
    client: &dyn CompletionClient,
    paragraph: &str,
    existing_notes: &str,
    prior: Option<KindState>,
    log: bool,
) -> Result<NotesOutcome, String> {
    let input_chars = paragraph.chars().count();
    log_step(
        log,
        "writer",
        &format!("paragraph={input_chars} chars, existing_notes={} chars", existing_notes.chars().count()),
    );

    let kind_state = classify(client, paragraph, existing_notes, prior, log);
    let kind = kind_state.kind.as_str();

    let writer_user = if existing_notes.trim().is_empty() {
        format!("Session kind: {kind}\n\nLatest transcript paragraph:\n{paragraph}")
    } else {
        format!(
            "Session kind: {kind}\n\nNotes so far:\n{existing_notes}\n\nLatest transcript paragraph:\n{paragraph}"
        )
    };
    let written = client.complete(WRITER_SYSTEM, &writer_user)?;
    log_step(log, "writer", &format!("returned {} chars", written.chars().count()));

    log_step(log, "validator", "checking writer notes against paragraph");
    let validator_user = format!(
        "Session kind: {kind}\n\nTranscript paragraph:\n{paragraph}\n\nDraft notes to validate:\n{written}"
    );
    let validated = client.complete(VALIDATOR_SYSTEM, &validator_user)?;
    log_step(log, "validator", &format!("returned {} chars", validated.chars().count()));

    Ok(NotesOutcome { notes: validated.trim().to_string(), kind: kind_state })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Replies per agent; records which agents ran and what each was given.
    struct Script {
        verdict: &'static str,
        calls: RefCell<Vec<(&'static str, String)>>,
    }
    impl Script {
        fn new(verdict: &'static str) -> Self {
            Self { verdict, calls: RefCell::new(Vec::new()) }
        }
        fn ran(&self, role: &str) -> usize {
            self.calls.borrow().iter().filter(|(r, _)| *r == role).count()
        }
    }
    impl CompletionClient for Script {
        fn complete(&self, system: &str, user: &str) -> Result<String, String> {
            let (role, reply) = if system.contains("KIND of spoken session") {
                ("classifier", self.verdict.to_string())
            } else if system.contains("meticulous note-taker") {
                ("writer", "writer saw input".to_string())
            } else {
                ("validator", "validator saw input".to_string())
            };
            self.calls.borrow_mut().push((role, user.to_string()));
            Ok(reply)
        }
    }

    const MEETING: &str = r#"{"kind":"meeting","confidence":"high","reason":"tasks assigned"}"#;

    fn v(kind: ConversationKind, confidence: Confidence) -> Verdict {
        Verdict { kind, confidence }
    }

    #[test]
    fn logging_defaults_off() {
        // Unset env in tests → logging disabled unless explicitly enabled.
        unsafe { std::env::remove_var(LOG_ENV_VAR) };
        assert!(!logging_enabled());
    }

    #[test]
    fn agent_runs_classifier_writer_then_validator() {
        let client = Script::new(MEETING);
        let out = agent_notes(&client, "new para", "old notes", None, false).unwrap();
        assert!(out.notes.contains("validator saw"), "got: {}", out.notes);
        assert!(!out.notes.contains("writer saw"), "final notes must come from validator");
        let order: Vec<_> = client.calls.borrow().iter().map(|(r, _)| *r).collect();
        assert_eq!(order, ["classifier", "writer", "validator"]);
    }

    #[test]
    fn agent_passes_empty_notes_on_first_run() {
        let out = agent_notes(&Script::new(MEETING), "first para", "", None, false).unwrap();
        assert!(out.notes.contains("validator saw"), "got: {}", out.notes);
    }

    #[test]
    fn writer_and_validator_receive_the_kind() {
        let client = Script::new(MEETING);
        agent_notes(&client, "p", "", None, false).unwrap();
        for (role, user) in client.calls.borrow().iter() {
            if *role != "classifier" {
                assert!(user.contains("Session kind: meeting"), "{role} got: {user}");
            }
        }
    }

    #[test]
    fn bad_classifier_reply_falls_back_to_conversation() {
        let out = agent_notes(&Script::new("sorry, no"), "p", "", None, false).unwrap();
        assert_eq!(out.kind.kind, ConversationKind::Conversation);
    }

    #[test]
    fn verdict_parses_through_code_fence() {
        let raw = format!("```json\n{MEETING}\n```");
        let got = parse_verdict(&raw).unwrap();
        assert_eq!(got.kind, ConversationKind::Meeting);
        assert_eq!(got.confidence, Confidence::High);
        assert!(parse_verdict(r#"{"kind":"standup"}"#).is_none());
    }

    #[test]
    fn classifier_stops_once_settled_and_rechecks_later() {
        let client = Script::new(MEETING);
        let mut state = None;
        for _ in 0..SETTLE_AFTER {
            state = Some(agent_notes(&client, "p", "", state, false).unwrap().kind);
        }
        assert_eq!(client.ran("classifier"), SETTLE_AFTER as usize);

        // Settled: the next RECHECK_EVERY paragraphs skip the classifier.
        for _ in 0..RECHECK_EVERY {
            state = Some(agent_notes(&client, "p", "", state, false).unwrap().kind);
        }
        assert_eq!(client.ran("classifier"), SETTLE_AFTER as usize);

        // Then it re-checks once.
        agent_notes(&client, "p", "", state, false).unwrap();
        assert_eq!(client.ran("classifier"), SETTLE_AFTER as usize + 1);
    }

    #[test]
    fn first_verdict_is_adopted() {
        let s = KindState::default().apply(v(ConversationKind::Meeting, Confidence::Medium), false);
        assert_eq!(s.kind, ConversationKind::Meeting);
        assert!(!s.settled());
    }

    #[test]
    fn low_confidence_never_moves_the_kind() {
        let s = KindState::default().apply(v(ConversationKind::Meeting, Confidence::High), false);
        let s = s.apply(v(ConversationKind::Presentation, Confidence::Low), true);
        assert_eq!(s.kind, ConversationKind::Meeting);
    }

    #[test]
    fn settled_kind_yields_only_to_high_confidence() {
        let mut s = KindState::default().apply(v(ConversationKind::Conversation, Confidence::High), false);
        while !s.settled() {
            s = s.apply(v(ConversationKind::Conversation, Confidence::High), true);
        }
        let s = s.apply(v(ConversationKind::Meeting, Confidence::Medium), true);
        assert_eq!(s.kind, ConversationKind::Conversation, "medium must not flip a settled kind");
        let s = s.apply(v(ConversationKind::Meeting, Confidence::High), true);
        assert_eq!(s.kind, ConversationKind::Meeting);
        assert!(!s.settled(), "a switch starts unsettled so it is re-verified");
    }

    #[test]
    fn prompts_are_embedded_and_agree_on_sections() {
        for section in ["## Action Items", "## Follow-ups", "## Open Questions"] {
            assert!(WRITER_SYSTEM.contains(section), "writer missing {section}");
        }
        for section in ["Action Items", "Follow-ups", "Open Questions"] {
            assert!(VALIDATOR_SYSTEM.contains(section), "validator missing {section}");
        }
        assert!(CLASSIFIER_SYSTEM.contains("conversation"));
    }
}
