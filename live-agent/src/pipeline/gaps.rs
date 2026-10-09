//! The closed vocabulary of gaps. The model classifies into this list; it is
//! never asked "is this important?", which small models answer badly.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GapType {
    SoftDeadline,
    Contradiction,
    HandedOffRisk,
    FuzzyCriteria,
    PlanNoFallback,
    UnsupportedClaim,
    NoNumber,
    DismissedAlternative,
    NoOwnerOrDate,
    Other,
}

pub struct GapSpec {
    /// What the question must uncover.
    pub intent: &'static str,
    /// What a good answer looks like (steers the model toward concrete asks).
    pub answer_type: &'static str,
    /// Weak patterns to avoid for this type.
    pub avoid: &'static [&'static str],
    /// Style examples from different domains. Never copied verbatim; two are
    /// shown per call, rotated.
    pub examples: &'static [&'static str],
}

const SOFT_DEADLINE: GapSpec = GapSpec {
    intent: "Find out whether the date is fixed and what event or driver sets it.",
    answer_type: "a date or event, and whether it can move",
    avoid: &[
        "'How firm is it?' (invites 'pretty firm', which tells you nothing)",
        "asking for the date again when one was already given",
    ],
    examples: &[
        "You said ideally by quarter end. Is there an event that sets that date?",
        "Is the renewal the real deadline, or can this slip?",
        "What happens if we deliver a week later?",
        "Does your start date depend on notice at your current job?",
    ],
};

const CONTRADICTION: GapSpec = GapSpec {
    intent: "Reconcile two statements that cannot both hold.",
    answer_type: "which statement holds, or what changed between them",
    avoid: &[
        "accusing tone or implying the speaker was wrong or dishonest",
        "paraphrasing either statement inaccurately; use the actual words of both",
    ],
    examples: &[
        "Earlier you said five people, now it sounds like twelve. Which should we plan on?",
        "You said the budget is fixed, but then mentioned some flexibility. Which is it?",
        "The date sounded firm earlier and now it's 'let's see'. Has something changed?",
    ],
};

const HANDED_OFF_RISK: GapSpec = GapSpec {
    intent: "Turn an unknown dependency handed to the other side into something concrete: what it is, how to reach it, who knows it.",
    answer_type: "a concrete fact about the dependency, or the name of a person who can provide it",
    avoid: &[
        "'What do you know about X?' (too open)",
        "'How old or complex is X?' (the answer 'old' changes nothing)",
        "asking whether it is 'a risk' or 'complicated'",
    ],
    examples: &[
        "You said your team can figure it out. Who on your side knows how it works today?",
        "What can we reach in that system: a database, an export, or just screens?",
        "Has the supplier confirmed the delivery in writing?",
        "Who can give us access to that, and have they agreed yet?",
    ],
};

const FUZZY_CRITERIA: GapSpec = GapSpec {
    intent: "Turn a vague requirement or go/no-go condition into something checkable.",
    answer_type: "a threshold with a unit, or a named condition someone could verify",
    avoid: &[
        "copying 'ready' or 'done' phrasing when the gap is about quality (fast, secure, simple)",
        "relative asks ('how much better?') when an absolute target is possible",
        "asking the speaker to predict what a third party would accept",
    ],
    examples: &[
        "What turnaround would make customers stop complaining?",
        "Has compliance written down what access has to look like?",
        "What would you need to see to say the pilot passed?",
        "What would a strong first six months in this role look like?",
    ],
};

const PLAN_NO_FALLBACK: GapSpec = GapSpec {
    intent: "Find the first failure point in a plan and what happens after it.",
    answer_type: "a named failure and the response (fallback, alert, owner)",
    avoid: &[
        "'What are the risks?' or 'What happens then?' without naming the failing step",
        "asking about several failures at once",
    ],
    examples: &[
        "If the supplier misses the delivery, what's the backup?",
        "If step two fails halfway, does step three still run?",
        "If your champion leaves mid-rollout, who takes over?",
        "If the first hire doesn't work out, what's the plan?",
    ],
};

const UNSUPPORTED_CLAIM: GapSpec = GapSpec {
    intent: "Get the specifics behind a claim of credit, capability or readiness that a decision rests on.",
    answer_type: "the speaker's own role or action, and one measurable result",
    avoid: &[
        "'Are you sure?' or 'Can you prove that?'",
        "'What's left before it's done?' (that is a status question, not a claim check)",
        "bundling role and result in one question",
    ],
    examples: &[
        "You said you led it. What was your part, compared with the team's?",
        "You said it went well. What changed that you could measure?",
        "You said it's basically done. What's the one thing still open?",
        "You said you've worked with clients like us. Which one is closest?",
    ],
};

const NO_NUMBER: GapSpec = GapSpec {
    intent: "Get the figure that scope, price or timeline depends on.",
    answer_type: "a figure with a unit, or a range",
    avoid: &[
        "'Roughly how much / how many / how long?' with no anchor in the speaker's words",
        "headcounts or volumes the speaker already sized roughly",
        "asking for a number nothing depends on",
    ],
    examples: &[
        "You said it should be cheaper. Cheaper by how much a month?",
        "You said setup takes a while. Is that days or weeks?",
        "What budget range are you working with?",
        "You said the team is stretched. How many hours a week can they give this?",
    ],
};

const DISMISSED_ALTERNATIVE: GapSpec = GapSpec {
    intent: "Learn why a simpler or obvious option was rejected or skipped.",
    answer_type: "a reason (cost, risk, constraint) or a trial result",
    avoid: &[
        "'Why not just Z?' in a challenging tone",
        "naming an alternative nobody mentioned unless it is the obvious one in context",
    ],
    examples: &[
        "Did you consider fixing what you have first? What ruled it out?",
        "You passed on the cheaper plan. What was missing in it?",
        "What made the in-house option a no?",
        "Did you try a smaller pilot before committing?",
    ],
};

const NO_OWNER_OR_DATE: GapSpec = GapSpec {
    intent: "Fill the single missing piece of an agreed action: the owner or the date.",
    answer_type: "a name OR a date, never both",
    avoid: &[
        "asking for both owner and date in one question",
        "asking for an owner when one is already named; ask for the date instead",
    ],
    examples: &[
        "Who's taking that on?",
        "You said someone from IT. Who in IT, by name?",
        "By when would you like to hear back from us?",
        "Who makes the final call on this?",
    ],
};

const OTHER: GapSpec = GapSpec {
    intent: "Resolve the unclear point described in the gap note.",
    answer_type: "whatever concrete fact the note says is missing",
    avoid: &["generic questions that could be pasted into any conversation"],
    examples: &[],
};

impl GapType {
    /// Types the understand stage may emit, in priority order (highest first).
    /// `Other` is a lowest-priority fallback and is not offered to the model.
    pub const ALL: [GapType; 9] = [
        GapType::SoftDeadline,
        GapType::HandedOffRisk,
        GapType::Contradiction,
        GapType::FuzzyCriteria,
        GapType::PlanNoFallback,
        GapType::UnsupportedClaim,
        GapType::NoNumber,
        GapType::DismissedAlternative,
        GapType::NoOwnerOrDate,
    ];

    /// Wire name, as used in the JSON schemas and prompts.
    pub fn name(self) -> &'static str {
        match self {
            GapType::SoftDeadline => "SOFT_DEADLINE",
            GapType::HandedOffRisk => "HANDED_OFF_RISK",
            GapType::Contradiction => "CONTRADICTION",
            GapType::FuzzyCriteria => "FUZZY_CRITERIA",
            GapType::PlanNoFallback => "PLAN_NO_FALLBACK",
            GapType::UnsupportedClaim => "UNSUPPORTED_CLAIM",
            GapType::NoNumber => "NO_NUMBER",
            GapType::DismissedAlternative => "DISMISSED_ALTERNATIVE",
            GapType::NoOwnerOrDate => "NO_OWNER_OR_DATE",
            GapType::Other => "OTHER",
        }
    }

    /// Lower is more important.
    pub fn priority(self) -> usize {
        Self::ALL.iter().position(|g| *g == self).unwrap_or(usize::MAX)
    }

    pub fn spec(self) -> &'static GapSpec {
        match self {
            GapType::SoftDeadline => &SOFT_DEADLINE,
            GapType::Contradiction => &CONTRADICTION,
            GapType::HandedOffRisk => &HANDED_OFF_RISK,
            GapType::FuzzyCriteria => &FUZZY_CRITERIA,
            GapType::PlanNoFallback => &PLAN_NO_FALLBACK,
            GapType::UnsupportedClaim => &UNSUPPORTED_CLAIM,
            GapType::NoNumber => &NO_NUMBER,
            GapType::DismissedAlternative => &DISMISSED_ALTERNATIVE,
            GapType::NoOwnerOrDate => &NO_OWNER_OR_DATE,
            GapType::Other => &OTHER,
        }
    }

    /// Rotate examples by chunk id so wording varies between calls.
    pub fn pick_examples(self, n: usize, seed: u32) -> Vec<&'static str> {
        let ex = self.spec().examples;
        if ex.is_empty() {
            return vec![];
        }
        (0..n.min(ex.len()))
            .map(|i| ex[(seed as usize + i) % ex.len()])
            .collect()
    }
}

pub struct WriteInput<'a> {
    pub gap_type: GapType,
    pub key_phrase: &'a str, // shortest span carrying the gap (from Stage 1)
    pub quote: &'a str,
    pub note: &'a str,       // Stage 1 "why it's a gap"
    pub speaker: &'a str,
    pub goal: &'a str,
    pub summary: &'a str,
    pub shown: &'a [String], // already suggested questions
    pub glossary: &'a str,
    pub chunk_id: u32,
}

fn bullets<I: IntoIterator<Item = S>, S: AsRef<str>>(items: I) -> String {
    items
        .into_iter()
        .map(|s| format!("- {}", s.as_ref()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_user_prompt(i: &WriteInput) -> String {
    let spec = i.gap_type.spec();
    let examples = i.gap_type.pick_examples(2, i.chunk_id);
    let shown = if i.shown.is_empty() {
        "(none)".to_string()
    } else {
        bullets(i.shown.iter().rev().take(10))
    };
    let goal = if i.goal.trim().is_empty() { "(not given)" } else { i.goal };
    let glossary = if i.glossary.trim().is_empty() { "(none)" } else { i.glossary };
    let examples_block = if examples.is_empty() {
        "(none)".to_string()
    } else {
        bullets(examples)
    };

    format!(
        "GAP TYPE: {}\n\
         WHAT THE QUESTION MUST UNCOVER: {}\n\
         ANSWER WE WANT: {}\n\n\
         KEY PHRASE: \"{}\"\n\
         FULL QUOTE: \"{}\"\n\
         WHY IT'S A GAP: {}\n\
         SPEAKER: {}\n\n\
         GOAL: {}\n\
         SUMMARY SO FAR: {}\n\
         GLOSSARY: {}\n\n\
         AVOID FOR THIS TYPE:\n{}\n\n\
         STYLE EXAMPLES (do not copy their wording):\n{}\n\n\
         ALREADY SUGGESTED (do not repeat or rephrase):\n{}",
        i.gap_type.name(), spec.intent, spec.answer_type, i.key_phrase, i.quote, i.note,
        i.speaker, goal, i.summary, glossary, bullets(spec.avoid), examples_block, shown
    )
}

/// The highest-priority item, so a chunk yields at most one question.
pub fn pick_best<T>(items: &[T], gap_type: impl Fn(&T) -> GapType) -> Option<&T> {
    items.iter().min_by_key(|i| gap_type(i).priority())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highest_priority_wins() {
        let gaps = [GapType::Other, GapType::NoOwnerOrDate, GapType::PlanNoFallback, GapType::NoNumber];
        assert_eq!(pick_best(&gaps, |g| *g), Some(&GapType::PlanNoFallback));
        assert_eq!(pick_best::<GapType>(&[], |g| *g), None);
    }

    #[test]
    fn wire_names_round_trip() {
        for g in GapType::ALL.into_iter().chain([GapType::Other]) {
            let json = serde_json::to_string(&g).unwrap();
            assert_eq!(json, format!("\"{}\"", g.name()));
        }
    }

    #[test]
    fn examples_rotate_and_other_has_none() {
        assert_eq!(GapType::NoNumber.pick_examples(2, 3).len(), 2);
        assert!(GapType::Other.pick_examples(2, 0).is_empty());
    }
}
