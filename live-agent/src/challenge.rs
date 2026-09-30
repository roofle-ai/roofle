//! AI follow-up-question ("challenge") generation — a three-agent flow.
//!
//! Each agent speaks the JSON schema in its prompt; this module parses those
//! responses into a structured [`ChallengeResult`] ready to persist:
//!
//! ```text
//! paragraph + open questions
//!   -> investigator (candidate questions)
//!   -> validator (approved questions)
//!   -> resolver (which open questions are answered / stale)
//!   -> ChallengeResult { added, answered, stale }
//! ```

use serde::Deserialize;

use crate::notes::CompletionClient;

/// Steers the investigator toward surfacing grounded, high-value follow-up
/// questions. Loaded from `prompts/challenge/investigator.txt` at compile time.
pub const INVESTIGATOR_SYSTEM: &str = include_str!("../prompts/challenge/investigator.txt");

/// Steers the validator toward keeping only questions that survive grounding,
/// value, dedup and conciseness checks.
pub const VALIDATOR_SYSTEM: &str = include_str!("../prompts/challenge/validator.txt");

/// Steers the resolver toward deciding which open questions are now answered
/// or stale. Loaded from `prompts/challenge/resolver.txt` at compile time.
pub const RESOLVER_SYSTEM: &str = include_str!("../prompts/challenge/resolver.txt");

/// What a challenge pass concluded, ready to persist.
#[derive(Debug, Default)]
pub struct ChallengeResult {
    /// Newly approved questions to insert as open.
    pub added: Vec<String>,
    /// Existing open question ids resolved as answered, with the answer.
    pub answered: Vec<(i64, String)>,
    /// Existing open question ids resolved as stale, with a short reason.
    pub stale: Vec<(i64, String)>,
}

/// Conversation context the agents use to avoid repeating ground and to tie
/// the newest chunk back to earlier claims. Either field may be empty.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChallengeContext<'a> {
    /// Compressed record of the conversation *before* the newest chunk (it
    /// must not already contain the newest chunk, or its content would look
    /// "already covered").
    pub summary: &'a str,
    /// Everything the user has said out loud (microphone), so questions they
    /// already asked or had answered are not suggested again.
    pub mic: &'a str,
}

/// Run the three-agent follow-up-question update for one paragraph boundary.
///
/// `paragraph` is the newly accumulated transcript chunk; `open_questions` are
/// the session's currently-open questions as `(id, text)`, so the resolver can
/// reference them by id. `context` carries the earlier-conversation summary and
/// the user's own speech. Returns the questions to add and the resolutions.
pub fn agent_challenges(
    client: &dyn CompletionClient,
    paragraph: &str,
    open_questions: &[(i64, String)],
    context: ChallengeContext<'_>,
    log: bool,
) -> Result<ChallengeResult, String> {
    crate::notes::log_step(
        log,
        "challenge/investigator",
        &format!(
            "paragraph={} chars, open_questions={}",
            paragraph.chars().count(),
            open_questions.len(),
        ),
    );

    // 1. Investigator: surface candidate questions from the new paragraph.
    let investigator_user = format!(
        "{}\n\nQuestions already open (do not duplicate):\n{}",
        context_block(paragraph, context),
        or_none(&numbered_with_ids(open_questions)),
    );
    let investigated = client.complete(INVESTIGATOR_SYSTEM, &investigator_user)?;
    crate::notes::log_step(
        log,
        "challenge/investigator",
        &format!("returned {} chars", investigated.chars().count()),
    );
    let inv: InvestigatorOut = parse_json(&investigated)?;
    let candidates: Vec<String> = inv
        .questions
        .into_iter()
        .filter(|q| !q.trim().is_empty())
        .collect();

    // 2. Validator: filter the drafts against the paragraph and context.
    let added: Vec<String> = if candidates.is_empty() {
        Vec::new()
    } else {
        crate::notes::log_step(log, "challenge/validator", "checking drafts against paragraph");
        let validator_user = format!(
            "{}\n\nInvestigator's reasoning:\n{}\n\nCandidate questions:\n{}",
            context_block(paragraph, context),
            or_none(inv.reasoning.trim()),
            numbered(&candidates),
        );
        let validated = client.complete(VALIDATOR_SYSTEM, &validator_user)?;
        crate::notes::log_step(
            log,
            "challenge/validator",
            &format!("returned {} chars", validated.chars().count()),
        );
        let val: ValidatorOut = parse_json(&validated)?;
        val.results
            .into_iter()
            .filter(|r| r.approved && !r.question.trim().is_empty())
            .map(|r| r.question.trim().to_string())
            .collect()
    };

    // 3. Resolver: decide which open questions the newest chunk resolves.
    let res: ResolverOut = if open_questions.is_empty() {
        ResolverOut::default()
    } else {
        crate::notes::log_step(log, "challenge/resolver", "resolving open questions");
        let resolver_user = format!(
            "{}\n\nOpen questions (each with its id):\n{}",
            context_block(paragraph, context),
            numbered_with_ids(open_questions),
        );
        let reviewed = client.complete(RESOLVER_SYSTEM, &resolver_user)?;
        crate::notes::log_step(
            log,
            "challenge/resolver",
            &format!("returned {} chars", reviewed.chars().count()),
        );
        parse_json(&reviewed)?
    };

    let answered = res
        .answered
        .iter()
        .filter_map(|e| parse_id(&e.id).map(|id| (id, e.reason.trim().to_string())))
        .collect();
    let stale = res
        .stale
        .iter()
        .filter_map(|e| parse_id(&e.id).map(|id| (id, e.reason.trim().to_string())))
        .collect();

    Ok(ChallengeResult {
        added,
        answered,
        stale,
    })
}

#[derive(Debug, Default, Deserialize)]
struct InvestigatorOut {
    #[serde(default)]
    questions: Vec<String>,
    #[serde(default)]
    reasoning: String,
}

#[derive(Debug, Default, Deserialize)]
struct ValidatorResult {
    #[serde(default)]
    question: String,
    #[serde(default)]
    approved: bool,
}

#[derive(Debug, Default, Deserialize)]
struct ValidatorOut {
    #[serde(default)]
    results: Vec<ValidatorResult>,
}

#[derive(Debug, Default, Deserialize)]
struct ResolveEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Default, Deserialize)]
struct ResolverOut {
    #[serde(default)]
    answered: Vec<ResolveEntry>,
    #[serde(default)]
    stale: Vec<ResolveEntry>,
}

/// Parse the agent's JSON, tolerating prose or a code fence around it.
fn parse_json<T: for<'de> Deserialize<'de>>(raw: &str) -> Result<T, String> {
    let start = raw.find('{').unwrap_or(0);
    let end = raw.rfind('}').map(|e| e + 1).unwrap_or(raw.len());
    serde_json::from_str(&raw[start..end]).map_err(|e| format!("bad challenge JSON: {e}"))
}

/// The three labelled inputs every agent receives, matching the headings the
/// prompts refer to.
fn context_block(paragraph: &str, context: ChallengeContext<'_>) -> String {
    format!(
        "NEWEST chunk (speaker transcription):\n{}\n\n\
         YOUR OWN microphone transcription (context):\n{}\n\n\
         RUNNING CONVERSATION SUMMARY (everything before the newest chunk):\n{}",
        paragraph.trim(),
        or_none(context.mic.trim()),
        or_none(context.summary.trim()),
    )
}

fn or_none(s: &str) -> &str {
    if s.is_empty() {
        "(none)"
    } else {
        s
    }
}

fn numbered(items: &[String]) -> String {
    items
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{}. {}", i + 1, s))
        .collect::<Vec<_>>()
        .join("\n")
}

fn numbered_with_ids(open: &[(i64, String)]) -> String {
    open.iter()
        .map(|(id, s)| format!("[id {}] {}", id, s))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extract the numeric part of a resolver-returned id (`"Q4"`, `"4"`, `" 4 "`).
fn parse_id(s: &str) -> Option<i64> {
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::CompletionClient;

    /// Returns a valid JSON response per stage, so tests observe parsing.
    struct JsonProbe;
    impl CompletionClient for JsonProbe {
        fn complete(&self, system: &str, _user: &str) -> Result<String, String> {
            if system == RESOLVER_SYSTEM {
                Ok(r#"{"answered":[{"id":"Q1","reason":"delivered by Friday"}],"stale":[{"id":"Q4","reason":"topic moved on"}]}"#.to_string())
            } else if system == INVESTIGATOR_SYSTEM {
                Ok(r#"{"questions":["Q1","Q2"],"reasoning":"probed assumptions"}"#.to_string())
            } else {
                Ok(r#"{"results":[{"question":"Q1","approved":true,"reason":"concrete"},{"question":"Q2","approved":false,"reason":"failed check 4"}],"summary":"1 approved"}"#.to_string())
            }
        }
    }

    #[test]
    fn parses_json_into_structured_result() {
        let out = agent_challenges(
            &JsonProbe,
            "para",
            &[(1, "open one".into()), (4, "open two".into())],
            ChallengeContext::default(),
            false,
        )
        .unwrap();
        assert_eq!(out.added, vec!["Q1".to_string()]);
        assert_eq!(out.answered, vec![(1, "delivered by Friday".to_string())]);
        assert_eq!(out.stale, vec![(4, "topic moved on".to_string())]);
    }

    #[test]
    fn tolerates_string_ids_with_prefix() {
        let id = parse_id("Q4");
        assert_eq!(id, Some(4));
        assert_eq!(parse_id("4"), Some(4));
        assert_eq!(parse_id("n/a"), None);
    }

    #[test]
    fn empty_questions_yield_empty_result() {
        struct EmptyProbe;
        impl CompletionClient for EmptyProbe {
            fn complete(&self, system: &str, _user: &str) -> Result<String, String> {
                if system == INVESTIGATOR_SYSTEM {
                    Ok(r#"{"questions":[],"reasoning":"nothing to probe"}"#.to_string())
                } else if system == RESOLVER_SYSTEM {
                    Ok(r#"{"answered":[],"stale":[]}"#.to_string())
                } else {
                    Ok(r#"{"results":[],"summary":"no candidates"}"#.to_string())
                }
            }
        }
        let out = agent_challenges(&EmptyProbe, "para", &[], ChallengeContext::default(), false).unwrap();
        assert!(out.added.is_empty());
        assert!(out.answered.is_empty());
        assert!(out.stale.is_empty());
    }

    #[test]
    fn agents_receive_summary_mic_and_open_questions() {
        use std::sync::Mutex;
        struct Recorder(Mutex<Vec<(String, String)>>);
        impl CompletionClient for Recorder {
            fn complete(&self, system: &str, user: &str) -> Result<String, String> {
                self.0.lock().unwrap().push((system.to_string(), user.to_string()));
                if system == RESOLVER_SYSTEM {
                    Ok(r#"{"answered":[],"stale":[]}"#.to_string())
                } else if system == INVESTIGATOR_SYSTEM {
                    Ok(r#"{"questions":["Q1"],"reasoning":"why"}"#.to_string())
                } else {
                    Ok(r#"{"results":[],"summary":""}"#.to_string())
                }
            }
        }
        let rec = Recorder(Mutex::new(Vec::new()));
        let ctx = ChallengeContext { summary: "EARLIER-SUMMARY", mic: "MY-OWN-WORDS" };
        agent_challenges(&rec, "NEW-CHUNK", &[(7, "open seven".into())], ctx, false).unwrap();

        let calls = rec.0.lock().unwrap();
        assert_eq!(calls.len(), 3, "investigator, validator and resolver all run");
        for (_, user) in calls.iter() {
            assert!(user.contains("NEW-CHUNK"), "{user}");
            assert!(user.contains("EARLIER-SUMMARY"), "{user}");
            assert!(user.contains("MY-OWN-WORDS"), "{user}");
        }
        assert!(calls[0].1.contains("[id 7] open seven"), "investigator sees open questions");
        assert!(calls[1].1.contains("why"), "validator sees investigator reasoning");
    }

    #[test]
    fn skips_validator_and_resolver_when_nothing_to_judge() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Counter(AtomicUsize);
        impl CompletionClient for Counter {
            fn complete(&self, _s: &str, _u: &str) -> Result<String, String> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(r#"{"questions":[],"reasoning":"none"}"#.to_string())
            }
        }
        let c = Counter(AtomicUsize::new(0));
        agent_challenges(&c, "para", &[], ChallengeContext::default(), false).unwrap();
        assert_eq!(c.0.load(Ordering::SeqCst), 1, "only the investigator runs");
    }
}
