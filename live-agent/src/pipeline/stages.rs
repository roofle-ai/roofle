//! Prompts and strict JSON schemas for the LLM stages. One job per call.

use serde::Deserialize;
use serde_json::{json, Value};

use super::gaps::GapType;
use super::memory::Speaker;

pub const UNDERSTAND_VERSION: &str = "understand.v2";
pub const WRITE_VERSION: &str = "write.v2";
pub const JUDGE_VERSION: &str = "judge.v2";

pub const UNDERSTAND_SYSTEM: &str = include_str!("../../prompts/pipeline/understand.v2.md");
pub const WRITE_SYSTEM: &str = include_str!("../../prompts/pipeline/write.v2.md");
pub const JUDGE_SYSTEM: &str = include_str!("../../prompts/pipeline/judge.v2.md");

#[derive(Debug, Clone, Deserialize)]
pub struct Gap {
    #[serde(rename = "type")]
    pub gap_type: GapType,
    pub quote: String,
    /// Shortest span of the quote that carries the gap.
    #[serde(default)]
    pub key_phrase: String,
    pub speaker: Speaker,
    pub source_id: String,
    pub note: String,
}

#[derive(Debug, Deserialize)]
pub struct Understand {
    pub clean_chunk: String,
    pub summary: String,
    pub answered_points: Vec<String>,
    pub answered_questions: Vec<String>,
    pub gaps: Vec<Gap>,
}

#[derive(Debug, Deserialize)]
pub struct Written {
    /// One-line reasoning (what the user would do differently per answer); logged, not shown.
    #[serde(default)]
    pub angle: String,
    pub question: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Verdict {
    pub answered: bool,
    pub evidence: String,
    pub duplicate: bool,
    pub generic: bool,
    pub detail_only: bool,
    pub goal_relevant: bool,
    pub high_impact: bool,
    pub outcome_change: String,
    /// 1 to 5 from the user's point of view; a reply without it is not held back.
    #[serde(default = "default_usefulness")]
    pub usefulness: u8,
}

/// Lowest usefulness score that still passes the judge.
pub const MIN_USEFULNESS: u8 = 3;

fn default_usefulness() -> u8 { MIN_USEFULNESS }

impl Verdict {
    /// The judge passes only when every check is clean and it can name an
    /// answer that would change the plan. `need_goal` adds the inferred-goal check.
    pub fn passes(&self, need_goal: bool) -> bool {
        !self.answered
            && !self.duplicate
            && !self.generic
            && !self.detail_only
            && self.outcome_change.trim().chars().count() > 15
            && self.usefulness >= MIN_USEFULNESS
            && (!need_goal || self.goal_relevant)
    }

    /// Short reason for the first failed check, for logs.
    pub fn failure(&self, need_goal: bool) -> &'static str {
        if self.answered { "answered" }
        else if self.duplicate { "duplicate" }
        else if self.generic { "generic" }
        else if self.detail_only { "detail_only" }
        else if need_goal && !self.goal_relevant { "not goal relevant" }
        else if self.outcome_change.trim().chars().count() <= 15 { "no outcome change" }
        else { "low usefulness" }
    }
}

fn strings() -> Value {
    json!({ "type": "array", "items": { "type": "string" } })
}

pub fn understand_schema() -> Value {
    let types: Vec<&str> = GapType::ALL.iter().map(|g| g.name()).collect();
    json!({
        "type": "object", "additionalProperties": false,
        "required": ["clean_chunk", "summary", "answered_points", "answered_questions", "gaps"],
        "properties": {
            "clean_chunk": { "type": "string" },
            "summary": { "type": "string" },
            "answered_points": strings(),
            "answered_questions": strings(),
            "gaps": { "type": "array", "items": {
                "type": "object", "additionalProperties": false,
                "required": ["type", "quote", "key_phrase", "speaker", "source_id", "note"],
                "properties": {
                    "type": { "type": "string", "enum": types },
                    "quote": { "type": "string" },
                    "key_phrase": { "type": "string" },
                    "speaker": { "type": "string", "enum": ["presenter", "other", "unknown"] },
                    "source_id": { "type": "string" },
                    "note": { "type": "string" }
                }
            }}
        }
    })
}

pub fn write_schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false,
        "required": ["angle", "question"],
        "properties": { "angle": { "type": "string" }, "question": { "type": ["string", "null"] } }
    })
}

pub fn judge_schema() -> Value {
    let b = json!({ "type": "boolean" });
    json!({
        "type": "object", "additionalProperties": false,
        "required": ["answered", "evidence", "duplicate", "generic", "detail_only",
                     "goal_relevant", "high_impact", "outcome_change", "usefulness"],
        "properties": {
            "answered": b, "evidence": { "type": "string" }, "duplicate": b,
            "generic": b, "detail_only": b, "goal_relevant": b, "high_impact": b,
            "outcome_change": { "type": "string" },
            "usefulness": { "type": "integer", "minimum": 1, "maximum": 5 }
        }
    })
}

/// Parse a model reply, tolerating prose or a code fence around the JSON.
pub fn parse<T: for<'de> Deserialize<'de>>(raw: &str) -> Result<T, String> {
    let start = raw.find('{').unwrap_or(0);
    let end = raw.rfind('}').map(|e| e + 1).unwrap_or(raw.len());
    serde_json::from_str(&raw[start..end]).map_err(|e| format!("bad stage JSON: {e}"))
}

/// `(none)` for an empty block, so prompts never show a blank section.
pub fn or_none(s: &str) -> &str {
    if s.trim().is_empty() { "(none)" } else { s }
}
