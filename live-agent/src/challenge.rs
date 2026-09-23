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

/// Run the three-agent follow-up-question update for one paragraph boundary.
///
/// `paragraph` is the newly accumulated transcript chunk; `open_questions` are
/// the session's currently-open questions as `(id, text)`, so the resolver can
/// reference them by id. Returns the questions to add and the resolutions.
pub fn agent_challenges(
    client: &dyn CompletionClient,
    paragraph: &str,
    open_questions: &[(i64, String)],
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
    let investigated = client.complete(INVESTIGATOR_SYSTEM, paragraph)?;
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

    // 2. Validator: filter the drafts against the paragraph.
    crate::notes::log_step(log, "challenge/validator", "checking drafts against paragraph");
    let validator_user = format!(
        "Transcript paragraph:\n{paragraph}\n\nCandidate questions:\n{}",
        numbered(&candidates),
    );
    let validated = client.complete(VALIDATOR_SYSTEM, &validator_user)?;
    crate::notes::log_step(
        log,
        "challenge/validator",
        &format!("returned {} chars", validated.chars().count()),
    );
    let val: ValidatorOut = parse_json(&validated)?;
    let added: Vec<String> = val
        .results
        .into_iter()
        .filter(|r| r.approved && !r.question.trim().is_empty())
        .map(|r| r.question.trim().to_string())
        .collect();

    // 3. Resolver: decide which open questions the newest chunk resolves.
    crate::notes::log_step(log, "challenge/resolver", "resolving open questions");
    let resolver_user = format!(
        "Transcript paragraph:\n{paragraph}\n\nOpen questions (each with its id):\n{}",
        numbered_with_ids(open_questions),
    );
    let reviewed = client.complete(RESOLVER_SYSTEM, &resolver_user)?;
    crate::notes::log_step(
        log,
        "challenge/resolver",
        &format!("returned {} chars", reviewed.chars().count()),
    );
    let res: ResolverOut = parse_json(&reviewed)?;

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
            if system.contains("resolver") {
                Ok(r#"{"answered":[{"id":"Q1","reason":"delivered by Friday"}],"stale":[{"id":"Q4","reason":"topic moved on"}]}"#.to_string())
            } else if system.contains("investigator") {
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
                if system.contains("investigator") {
                    Ok(r#"{"questions":[],"reasoning":"nothing to probe"}"#.to_string())
                } else if system.contains("resolver") {
                    Ok(r#"{"answered":[],"stale":[]}"#.to_string())
                } else {
                    Ok(r#"{"results":[],"summary":"no candidates"}"#.to_string())
                }
            }
        }
        let out = agent_challenges(&EmptyProbe, "para", &[], false).unwrap();
        assert!(out.added.is_empty());
        assert!(out.answered.is_empty());
        assert!(out.stale.is_empty());
    }
}
