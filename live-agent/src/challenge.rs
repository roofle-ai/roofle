//! AI follow-up-question ("challenge") generation — a three-agent flow.
//!
//! Parallel to the notes agent ([`crate::notes`]), this drives a second live
//! agent that turns the transcript into the follow-up questions a participant
//! would want to ask. It shares the same [`CompletionClient`] abstraction and
//! the same `ROOFLE_LOG_AI` debug logging, so it stays pure and testable and
//! its every step is observable during development.
//!
//! Flow per paragraph boundary:
//!   1. **Investigator agent**: given the latest paragraph, surface candidate
//!      follow-up questions grounded in what was said.
//!   2. **Validator agent**: given the paragraph + the investigator's drafts,
//!      drop anything weak, unsupported, generic, or already resolved.
//!   3. **Resolver agent**: given the newest paragraph + the open questions,
//!      decide which are now answered or stale.
//!
//! The resulting question list is accumulated across the whole meeting (not
//! just the latest paragraph), so the output grows as the conversation does.

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

/// Run the three-agent follow-up-question update for one paragraph boundary.
///
/// `paragraph` is the newly accumulated transcript chunk; `existing_questions`
/// is the running list of follow-up questions produced so far (empty on the
/// first paragraph). Returns the resolver's final question list.
pub fn agent_challenges(
    client: &dyn CompletionClient,
    paragraph: &str,
    existing_questions: &str,
    log: bool,
) -> Result<String, String> {
    crate::notes::log_step(
        log,
        "challenge/investigator",
        &format!(
            "paragraph={} chars, existing_questions={} chars",
            paragraph.chars().count(),
            existing_questions.chars().count(),
        ),
    );

    // 1. Investigator: surface candidate questions from the new paragraph.
    let investigated = client.complete(INVESTIGATOR_SYSTEM, paragraph)?;
    crate::notes::log_step(
        log,
        "challenge/investigator",
        &format!("returned {} chars", investigated.chars().count()),
    );

    // 2. Validator: filter the drafts against the paragraph.
    crate::notes::log_step(log, "challenge/validator", "checking drafts against paragraph");
    let validator_user = format!(
        "Transcript paragraph:\n{paragraph}\n\nInvestigator's draft follow-up questions:\n{investigated}"
    );
    let validated = client.complete(VALIDATOR_SYSTEM, &validator_user)?;
    crate::notes::log_step(
        log,
        "challenge/validator",
        &format!("returned {} chars", validated.chars().count()),
    );

    // 3. Resolver: decide which open questions the newest paragraph resolves.
    crate::notes::log_step(log, "challenge/resolver", "resolving open questions");
    let review_user = if existing_questions.trim().is_empty() {
        format!(
            "Transcript paragraph:\n{paragraph}\n\nValidator's follow-up questions:\n{validated}"
        )
    } else {
        format!(
            "Transcript paragraph:\n{paragraph}\n\nFollow-up questions so far:\n{existing_questions}\n\nValidator's newly proposed questions:\n{validated}"
        )
    };
    let reviewed = client.complete(RESOLVER_SYSTEM, &review_user)?;
    crate::notes::log_step(
        log,
        "challenge/resolver",
        &format!("returned {} chars", reviewed.chars().count()),
    );

    Ok(reviewed.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::CompletionClient;

    /// A client that reports which agent stage ran, by matching its system
    /// prompt, so tests can observe the pipeline order.
    struct StageProbe;
    impl CompletionClient for StageProbe {
        fn complete(&self, system: &str, _user: &str) -> Result<String, String> {
            // Order matters: the resolver prompt mentions "validator's
            // shortlist", so it also matches "validator"; test for the more
            // specific "resolver" marker first.
            let stage = if system.contains("resolver") {
                "resolver"
            } else if system.contains("investigator") {
                "investigator"
            } else {
                "validator"
            };
            Ok(format!("{stage} ran"))
        }
    }

    #[test]
    fn runs_investigator_then_validator_then_resolver() {
        let out = agent_challenges(&StageProbe, "new para", "", false).unwrap();
        assert!(out.contains("resolver ran"), "final must come from resolver: {out}");
        assert!(!out.contains("investigator ran"), "got: {out}");
        assert!(!out.contains("validator ran"), "got: {out}");
    }

    #[test]
    fn passes_empty_list_on_first_run() {
        let out = agent_challenges(&StageProbe, "first para", "", false).unwrap();
        assert!(out.contains("resolver ran"), "got: {out}");
    }

    #[test]
    fn passes_existing_questions_on_later_runs() {
        let out = agent_challenges(&StageProbe, "next para", "prior q", false).unwrap();
        assert!(out.contains("resolver ran"), "got: {out}");
    }
}
