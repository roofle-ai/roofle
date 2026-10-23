//! Code-side rules the model forgets. Each returns `Err(reason)` so the
//! drop reason can be logged; silence is cheaper than a wrong question.

use std::collections::HashSet;

use super::memory::{GlossaryEntry, Speaker};

const BANNED_OPENINGS: [&str; 7] = [
    "what specific",
    "what has to be true",
    "can you explain",
    "can you elaborate",
    "can you clarify",
    "can you tell me more",
    "could you please",
];
const QUESTION_WORDS: [&str; 12] =
    ["what", "how", "why", "when", "who", "which", "where", "would", "do", "does", "is", "are"];
const MIN_WORDS: usize = 6;
const MAX_WORDS: usize = 30;
pub const DEDUP_THRESHOLD: f64 = 0.8;

/// Check a drafted question. `gap_speaker` is who said the gap; `recent` are
/// the last questions asked (any state). `ban_openings` also rejects stock
/// openings; the persona path leaves that call to the judge.
pub fn check(
    text: &str,
    gap_speaker: Speaker,
    glossary: &[GlossaryEntry],
    recent: &[&str],
    ban_openings: bool,
) -> Result<(), String> {
    let lower = text.trim().to_lowercase();

    let words = lower.split_whitespace().count();
    if !(MIN_WORDS..=MAX_WORDS).contains(&words) {
        return Err(format!("length: {words} words"));
    }
    if let Some(b) = BANNED_OPENINGS.iter().filter(|_| ban_openings).find(|b| lower.starts_with(**b)) {
        return Err(format!("banned opening: {b}"));
    }
    if lower.matches('?').count() > 1 {
        return Err("more than one question mark".into());
    }
    // " and " joining two question clauses = two asks.
    let joins_clause = lower
        .split(" and ")
        .skip(1)
        .any(|rest| rest.split_whitespace().next().is_some_and(|w| QUESTION_WORDS.contains(&w)));
    if joins_clause {
        return Err("two asks joined by 'and'".into());
    }
    if gap_speaker != Speaker::Presenter
        && (lower.starts_with("you mentioned") || lower.starts_with("you said"))
    {
        return Err("misattributed: other party said it".into());
    }
    for g in glossary {
        if let Some(h) = g.heard_as.iter().find(|h| !h.is_empty() && lower.contains(&h.to_lowercase())) {
            return Err(format!("garbled term: {h}"));
        }
    }
    if let Some(r) = recent.iter().find(|r| jaccard(&lower, r) > DEDUP_THRESHOLD) {
        return Err(format!("duplicate of: {r}"));
    }
    Ok(())
}

fn tokens(s: &str) -> HashSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Token-set Jaccard similarity, 0.0 to 1.0.
// ponytail: token overlap misses paraphrases; swap for embeddings if duplicates leak through.
pub fn jaccard(a: &str, b: &str) -> f64 {
    let (a, b) = (tokens(a), tokens(b));
    let union = a.union(&b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(&b).count() as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "What happens if the Glue job succeeds but the dbt trigger fails?";

    #[test]
    fn good_question_passes() {
        assert_eq!(check(GOOD, Speaker::Presenter, &[], &[], true), Ok(()));
    }

    #[test]
    fn rejects_known_bad_shapes() {
        let bad = |t: &str| check(t, Speaker::Presenter, &[], &[], true).is_err();
        assert!(bad("What specific timeframe are you referring to for the switchover?"));
        assert!(bad("What has to be true for the new pipeline to count as ready?"));
        assert!(bad("Why?"));
        assert!(bad("What is the cost? And who pays for it eventually here?"));
        assert!(bad("What is the annual cost and who owns the budget for it?"));
        assert!(!bad("What is the annual cost of Glue and Fivetran at our volume?"));
    }

    #[test]
    fn attribution_and_glossary_and_dedup() {
        assert!(check("You mentioned the purge needs table config changes, correct?", Speaker::Other, &[], &[], true).is_err());
        let g = [GlossaryEntry { term: "Fivetran".into(), heard_as: vec!["5-tran".into()] }];
        assert!(check("What does running 5-tran in hybrid mode cost per year?", Speaker::Presenter, &g, &[], true).is_err());
        assert!(check(GOOD, Speaker::Presenter, &[], &[GOOD], true).is_err());
        assert!(check(GOOD, Speaker::Presenter, &[], &["What is the annual cost of Fivetran versus Glue?"], true).is_ok());
    }
}
