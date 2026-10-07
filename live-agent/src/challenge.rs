//! AI follow-up-question ("challenge") generation — a three-agent flow.
//!
//! Each agent speaks the JSON schema in its prompt; this module parses those
//! responses into a structured [`ChallengeResult`] ready to persist:
//!
//! ```text
//! paragraph + open questions + previous ledger
//!   -> ledger (folds the paragraph into the open vague points)
//!   -> investigator (scores candidates, returns 0 or 1 question)
//!   -> validator (grounding, duplicates, speakable; ignores importance)
//!   -> resolver (which open questions are answered / stale)
//!   -> ChallengeResult { added, answered, stale, ledger }
//! ```
//!
//! The ledger runs first, so the investigator, validator and resolver all see
//! the vague points *including* the newest chunk. Each stage has one job: the investigator alone judges importance, the
//! validator never sees its reasoning (so it stays an independent check), and
//! the resolver only marks a question answered when it can quote the answer.

use serde::Deserialize;

use crate::notes::CompletionClient;

/// Steers the investigator toward surfacing grounded, high-value follow-up
/// questions. Loaded from `prompts/challenge/investigator.txt` at compile time.
pub const INVESTIGATOR_SYSTEM: &str = include_str!("../prompts/challenge/investigator.txt");

/// Steers the validator toward mechanical checks only (grounding, already
/// asked or answered, duplicates, single target); importance is the
/// investigator's call.
pub const VALIDATOR_SYSTEM: &str = include_str!("../prompts/challenge/validator.txt");

/// Steers the resolver toward deciding which open questions are now answered
/// or stale. Loaded from `prompts/challenge/resolver.txt` at compile time.
pub const RESOLVER_SYSTEM: &str = include_str!("../prompts/challenge/resolver.txt");

/// Steers the ledger agent toward keeping a short list of statements that are
/// still vague or unresolved. Loaded from `prompts/challenge/ledger.txt`.
pub const LEDGER_SYSTEM: &str = include_str!("../prompts/challenge/ledger.txt");

/// The ledger never grows past this many points, whatever the model returns.
const MAX_LEDGER_POINTS: usize = 8;

/// The investigator returns at most this many questions per chunk; silence is
/// the expected result, so a pass never adds more than one.
const MAX_QUESTIONS_PER_CHUNK: usize = 1;

/// A newly approved question and the transcript entry it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedQuestion {
    pub text: String,
    /// `transcript_log` sequence of the entry the question is anchored in:
    /// the newest chunk, or an earlier entry it refers back to.
    pub source_seq: i64,
}

/// What a challenge pass concluded, ready to persist.
#[derive(Debug, Default)]
pub struct ChallengeResult {
    /// Newly approved questions to insert as open.
    pub added: Vec<AddedQuestion>,
    /// Existing open question ids resolved as answered, with the answer.
    pub answered: Vec<(i64, String)>,
    /// Existing open question ids resolved as stale, with a short reason.
    pub stale: Vec<(i64, String)>,
    /// The open vague points after this chunk, to carry into the next one, or
    /// `None` when the ledger step failed and the previous ledger is kept.
    pub ledger: Option<Vec<String>>,
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
    /// Open vague points from *before* the newest chunk, one per line (empty on
    /// the first chunk). The ledger agent updates them with the newest chunk;
    /// the other agents receive the updated list.
    pub ledger: &'a str,
}

/// Run the three-agent follow-up-question update for one paragraph boundary.
///
/// `paragraph` is the newly accumulated transcript chunk, logged as entry
/// `seq`; every transcript entry the agents see carries its `[T<seq>]` tag so
/// the investigator can name the entry a question comes from. `open_questions` are
/// the session's currently-open questions as `(id, text)`, so the resolver can
/// reference them by id. `context` carries the earlier-conversation summary and
/// the user's own speech. Returns the questions to add and the resolutions.
pub fn agent_challenges(
    client: &dyn CompletionClient,
    paragraph: &str,
    seq: i64,
    open_questions: &[(i64, String)],
    context: ChallengeContext<'_>,
    log: bool,
) -> Result<ChallengeResult, String> {
    let tagged = tag_entry(seq, paragraph);
    let paragraph = tagged.as_str();

    // 0. Ledger: fold the newest chunk into the open vague points first, so
    // every later agent sees them. If it fails, carry on with the previous
    // ledger and report `None` so the caller keeps it.
    let ledger = update_ledger(client, paragraph, context, log);
    let ledger_text = match &ledger {
        Some(points) => format_ledger(points),
        None => context.ledger.to_string(),
    };
    let context = ChallengeContext { ledger: &ledger_text, ..context };

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
    crate::notes::log_step(
        log,
        "challenge/investigator",
        &format!("reasoning: {}", inv.reasoning.trim()),
    );
    let candidates: Vec<(String, i64)> = inv
        .questions
        .into_iter()
        .map(|q| {
            let (text, source) = q.into_parts();
            // A missing or out-of-range id falls back to the newest chunk, which
            // every question must trace to anyway.
            let source_seq = source.filter(|id| (1..=seq).contains(id)).unwrap_or(seq);
            (text.trim().to_string(), source_seq)
        })
        .filter(|(text, _)| !text.is_empty())
        .take(MAX_QUESTIONS_PER_CHUNK)
        .collect();

    // 2. Validator: filter the drafts against the paragraph and context.
    let added: Vec<AddedQuestion> = if candidates.is_empty() {
        Vec::new()
    } else {
        crate::notes::log_step(log, "challenge/validator", "checking drafts against paragraph");
        // The investigator's reasoning is withheld on purpose: seeing it makes
        // the validator inclined to agree, which weakens the independent check.
        let validator_user = format!(
            "{}\n\nQuestions already open (do not approve a duplicate):\n{}\n\nCandidate questions:\n{}",
            context_block(paragraph, context),
            or_none(&numbered_with_ids(open_questions)),
            numbered(&candidates.iter().map(|(text, _)| text.clone()).collect::<Vec<_>>()),
        );
        let validated = client.complete(VALIDATOR_SYSTEM, &validator_user)?;
        crate::notes::log_step(
            log,
            "challenge/validator",
            &format!("returned {} chars", validated.chars().count()),
        );
        let val: ValidatorOut = parse_json(&validated)?;
        for r in val.results.iter().filter(|r| !r.approved) {
            crate::notes::log_step(
                log,
                "challenge/validator",
                &format!("rejected \"{}\": {}", r.question.trim(), r.reason.trim()),
            );
        }
        val.results
            .into_iter()
            .filter(|r| r.approved && !r.question.trim().is_empty())
            .map(|r| {
                let text = r.question.trim().to_string();
                let source_seq = candidates
                    .iter()
                    .find(|(candidate, _)| *candidate == text)
                    .map_or(seq, |(_, source)| *source);
                AddedQuestion { text, source_seq }
            })
            .take(MAX_QUESTIONS_PER_CHUNK)
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

    for e in &res.partial {
        crate::notes::log_step(
            log,
            "challenge/resolver",
            &format!("partial {}: {}", e.id, e.reason.trim()),
        );
    }
    // An answer without a quoted `evidence` is not trusted: the question stays
    // open, since wrongly hiding a real gap costs more than a glance.
    let answered = res
        .answered
        .iter()
        .filter(|e| !e.evidence.trim().is_empty())
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
        ledger,
    })
}

/// Ask the ledger agent for the open vague points after the newest chunk.
/// `None` on any failure or unusable reply, so the caller keeps the old ledger.
fn update_ledger(
    client: &dyn CompletionClient,
    paragraph: &str,
    context: ChallengeContext<'_>,
    log: bool,
) -> Option<Vec<String>> {
    let user = format!(
        "NEWEST chunk (speaker transcription):\n{}\n\n\
         YOUR OWN microphone transcription (context):\n{}\n\n\
         CURRENT ledger:\n{}",
        paragraph.trim(),
        or_none(context.mic.trim()),
        or_none(context.ledger.trim()),
    );
    let out: LedgerOut = match client
        .complete(LEDGER_SYSTEM, &user)
        .and_then(|raw| parse_json(&raw))
    {
        Ok(out) => out,
        Err(e) => {
            crate::notes::log_step(log, "challenge/ledger", &format!("kept old ledger: {e}"));
            return None;
        }
    };
    let points: Vec<String> = out
        .points?
        .into_iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .take(MAX_LEDGER_POINTS)
        .collect();
    crate::notes::log_step(log, "challenge/ledger", &format!("{} open vague points", points.len()));
    Some(points)
}

/// One ledger point per line, numbered, for the agents' prompts.
pub fn format_ledger(points: &[String]) -> String {
    points
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{}. {}", i + 1, p))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Default, Deserialize)]
struct LedgerOut {
    /// Absent (not merely empty) means the reply was not a ledger.
    #[serde(default)]
    points: Option<Vec<String>>,
}

/// A candidate question: a bare string, or an object naming its source entry.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum InvestigatorQuestion {
    Plain(String),
    Sourced {
        question: String,
        #[serde(default)]
        source_id: Option<serde_json::Value>,
    },
}

impl InvestigatorQuestion {
    fn into_parts(self) -> (String, Option<i64>) {
        match self {
            Self::Plain(text) => (text, None),
            Self::Sourced { question, source_id } => {
                let id = source_id.and_then(|v| match v {
                    serde_json::Value::Number(n) => n.as_i64(),
                    serde_json::Value::String(s) => parse_id(&s),
                    _ => None,
                });
                (question, id)
            }
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct InvestigatorOut {
    #[serde(default)]
    questions: Vec<InvestigatorQuestion>,
    #[serde(default)]
    reasoning: String,
}

#[derive(Debug, Default, Deserialize)]
struct ValidatorResult {
    #[serde(default)]
    question: String,
    #[serde(default)]
    approved: bool,
    #[serde(default)]
    reason: String,
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
    /// Verbatim words that answer the question; required for `answered`.
    #[serde(default)]
    evidence: String,
}

#[derive(Debug, Default, Deserialize)]
struct ResolverOut {
    #[serde(default)]
    answered: Vec<ResolveEntry>,
    #[serde(default)]
    stale: Vec<ResolveEntry>,
    /// Questions that got a partial reply and stay open (logged only).
    #[serde(default)]
    partial: Vec<ResolveEntry>,
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
         RUNNING CONVERSATION SUMMARY (everything before the newest chunk):\n{}\n\n\
         OPEN VAGUE POINTS (still unresolved, updated with the newest chunk):\n{}",
        paragraph.trim(),
        or_none(context.mic.trim()),
        or_none(context.summary.trim()),
        or_none(context.ledger.trim()),
    )
}

/// Prefix a transcript entry with its `[T<seq>]` tag, the id agents cite.
pub fn tag_entry(seq: i64, text: &str) -> String {
    format!("[T{seq}] {}", text.trim())
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
            if system == LEDGER_SYSTEM {
                Ok(r#"{"points":["\"fast\": no number"]}"#.to_string())
            } else if system == RESOLVER_SYSTEM {
                Ok(r#"{"answered":[{"id":"Q1","evidence":"by Friday","reason":"delivered by Friday"},{"id":"Q4","reason":"no evidence quoted"}],"stale":[{"id":"Q4","reason":"topic moved on"}],"partial":[{"id":"Q2","reason":"role only"}]}"#.to_string())
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
            9,
            &[(1, "open one".into()), (4, "open two".into())],
            ChallengeContext::default(),
            false,
        )
        .unwrap();
        assert_eq!(out.added, vec![AddedQuestion { text: "Q1".into(), source_seq: 9 }]);
        assert_eq!(
            out.answered,
            vec![(1, "delivered by Friday".to_string())],
            "an answered entry without evidence is ignored"
        );
        assert_eq!(out.stale, vec![(4, "topic moved on".to_string())]);
        assert_eq!(out.ledger, Some(vec!["\"fast\": no number".to_string()]));
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
        let out = agent_challenges(&EmptyProbe, "para", 1, &[], ChallengeContext::default(), false).unwrap();
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
                } else if system == LEDGER_SYSTEM {
                    Ok(r#"{"points":["\"fast\": no number"]}"#.to_string())
                } else if system == INVESTIGATOR_SYSTEM {
                    Ok(r#"{"questions":["Q1"],"reasoning":"why"}"#.to_string())
                } else {
                    Ok(r#"{"results":[],"summary":""}"#.to_string())
                }
            }
        }
        let rec = Recorder(Mutex::new(Vec::new()));
        let ctx = ChallengeContext {
            summary: "EARLIER-SUMMARY",
            mic: "MY-OWN-WORDS",
            ledger: "1. \"soon\": no date",
        };
        agent_challenges(&rec, "NEW-CHUNK", 5, &[(7, "open seven".into())], ctx, false).unwrap();

        let calls = rec.0.lock().unwrap();
        assert_eq!(calls.len(), 4, "ledger, investigator, validator and resolver all run");
        assert_eq!(calls[0].0, LEDGER_SYSTEM, "the ledger runs first");
        assert!(calls[0].1.contains("\"soon\": no date"), "ledger sees the previous ledger");
        for (_, user) in calls.iter() {
            assert!(user.contains("NEW-CHUNK"), "{user}");
            assert!(user.contains("MY-OWN-WORDS"), "{user}");
        }
        for (_, user) in calls.iter().skip(1) {
            assert!(user.contains("EARLIER-SUMMARY"), "{user}");
        }
        assert!(calls[1].1.contains("[id 7] open seven"), "investigator sees open questions");
        assert!(!calls[2].1.contains("why"), "validator must not see investigator reasoning");
        assert!(calls[2].1.contains("[id 7] open seven"), "validator sees open questions");
        for (i, call) in calls.iter().enumerate().skip(1) {
            assert!(call.1.contains("fast"), "stage {i} sees the UPDATED ledger: {}", call.1);
        }
    }

    #[test]
    fn at_most_one_question_is_added() {
        // JsonProbe's investigator proposes two and its validator approves Q1 only,
        // so also check the investigator cap on its own.
        struct Two;
        impl CompletionClient for Two {
            fn complete(&self, system: &str, _user: &str) -> Result<String, String> {
                if system == INVESTIGATOR_SYSTEM {
                    Ok(r#"{"questions":["A","B"],"reasoning":"x"}"#.to_string())
                } else {
                    Ok(r#"{"results":[{"question":"A","approved":true},{"question":"B","approved":true}]}"#.to_string())
                }
            }
        }
        let out = agent_challenges(&Two, "para", 1, &[], ChallengeContext::default(), false).unwrap();
        assert_eq!(out.added, vec![AddedQuestion { text: "A".into(), source_seq: 1 }]);
    }

    #[test]
    fn failed_ledger_keeps_the_old_one_and_the_rest_still_runs() {
        use std::sync::Mutex;
        struct Junk(Mutex<Vec<String>>);
        impl CompletionClient for Junk {
            fn complete(&self, system: &str, user: &str) -> Result<String, String> {
                if system == LEDGER_SYSTEM {
                    Ok("not json".to_string())
                } else {
                    self.0.lock().unwrap().push(user.to_string());
                    Ok(r#"{"questions":[],"reasoning":"none"}"#.to_string())
                }
            }
        }
        let c = Junk(Mutex::new(Vec::new()));
        let ctx = ChallengeContext { ledger: "1. \"old\": missing", ..Default::default() };
        let out = agent_challenges(&c, "para", 1, &[], ctx, false).unwrap();
        assert_eq!(out.ledger, None);
        assert!(c.0.lock().unwrap()[0].contains("\"old\": missing"), "investigator falls back to the old ledger");
    }

    #[test]
    fn ledger_is_capped() {
        struct Many;
        impl CompletionClient for Many {
            fn complete(&self, system: &str, _user: &str) -> Result<String, String> {
                if system == LEDGER_SYSTEM {
                    let pts: Vec<String> = (0..12).map(|i| format!("p{i}")).collect();
                    Ok(serde_json::json!({ "points": pts }).to_string())
                } else {
                    Ok(r#"{"questions":[],"reasoning":"none"}"#.to_string())
                }
            }
        }
        let out = agent_challenges(&Many, "para", 1, &[], ChallengeContext::default(), false).unwrap();
        assert_eq!(out.ledger.unwrap().len(), MAX_LEDGER_POINTS);
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
        agent_challenges(&c, "para", 1, &[], ChallengeContext::default(), false).unwrap();
        assert_eq!(c.0.load(Ordering::SeqCst), 2, "only the ledger and investigator run");
    }

    #[test]
    fn newest_chunk_is_tagged_and_question_keeps_its_cited_source() {
        use std::sync::Mutex;
        struct Cites(Mutex<Vec<String>>, &'static str);
        impl CompletionClient for Cites {
            fn complete(&self, system: &str, user: &str) -> Result<String, String> {
                if system == INVESTIGATOR_SYSTEM {
                    self.0.lock().unwrap().push(user.to_string());
                    Ok(self.1.to_string())
                } else if system == LEDGER_SYSTEM {
                    Ok(r#"{"points":[]}"#.to_string())
                } else {
                    Ok(r#"{"results":[{"question":"Q?","approved":true}]}"#.to_string())
                }
            }
        }
        let run = |reply: &'static str| {
            let c = Cites(Mutex::new(Vec::new()), reply);
            let out = agent_challenges(&c, "NEW", 6, &[], ChallengeContext::default(), false).unwrap();
            let prompt = c.0.lock().unwrap()[0].clone();
            (out.added, prompt)
        };

        let (added, prompt) = run(r#"{"questions":[{"question":"Q?","source_id":"T3"}],"reasoning":"x"}"#);
        assert!(prompt.contains("[T6] NEW"), "{prompt}");
        assert_eq!(added[0].source_seq, 3, "an earlier cited entry is kept");

        let (added, _) = run(r#"{"questions":[{"question":"Q?","source_id":99}],"reasoning":"x"}"#);
        assert_eq!(added[0].source_seq, 6, "an unknown id falls back to the newest chunk");

        let (added, _) = run(r#"{"questions":["Q?"],"reasoning":"x"}"#);
        assert_eq!(added[0].source_seq, 6, "a bare string falls back to the newest chunk");
    }
}
