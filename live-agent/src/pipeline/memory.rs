//! Per-session memory. The model is stateless: this struct is the only state,
//! owned by one session worker and injected into every prompt.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::gaps::GapType;

pub const SCHEMA_VERSION: u32 = 1;
pub const SUMMARY_MAX_CHARS: usize = 1_200;
pub const MAX_OPEN_POINTS: usize = 15;
pub const MAX_GLOSSARY: usize = 50;
pub const RECENT_CHUNKS: usize = 2;
/// How many past questions are shown to the model and compared for duplicates.
pub const DEDUP_WINDOW: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeetingType {
    Decision,
    Review,
    Sales,
    Status,
    Interview,
    Discovery,
    Negotiation,
    Planning,
    Training,
    Support,
    #[default]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    /// The party being questioned (system audio in the app).
    Presenter,
    Other,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionState {
    Pending,
    Open,
    Resolved,
    Stale,
    Dismissed,
    Asked,
}

impl QuestionState {
    /// Whether a question in this state still counts as live (not yet shown
    /// as answered or discarded).
    pub fn is_live(self) -> bool {
        matches!(self, QuestionState::Pending | QuestionState::Open)
    }

    fn can_become(self, to: QuestionState) -> bool {
        use QuestionState::*;
        matches!(
            (self, to),
            (Pending, Open)
                | (Pending, Dismissed)
                | (Open, Resolved)
                | (Open, Stale)
                | (Open, Dismissed)
                | (Open, Asked)
                | (Stale, Resolved)
                | (Stale, Dismissed)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlossaryEntry {
    pub term: String,
    pub heard_as: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenPoint {
    pub id: String,
    /// Exact quote.
    pub text: String,
    pub gap_type: GapType,
    pub speaker: Speaker,
    pub chunk_id: u32,
    pub source_ts_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub id: String,
    pub text: String,
    pub gap_type: GapType,
    /// Time in the call of the line that raised it (what the UI shows).
    pub source_ts_ms: i64,
    /// Chunk (transcript paragraph) the question was raised from.
    pub source_chunk: u32,
    pub state: QuestionState,
    pub created_at_ms: i64,
    pub ready_at_ms: i64,
    pub opened_at_ms: Option<i64>,
    pub resolved_by_chunk: Option<u32>,
    /// The judge's "one plausible answer that would change the plan".
    pub outcome_change: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Memory {
    pub session_id: String,
    /// The decision or outcome at stake (user-set or inferred).
    pub goal: String,
    pub goal_inferred: bool,
    pub meeting_type: MeetingType,
    /// Who the user is; rendered into the write prompt with the current goal.
    #[serde(default)]
    pub persona: roofle_core::persona::PersonaProfile,
    pub glossary: Vec<GlossaryEntry>,
    /// Rolling, rewritten each chunk.
    pub summary: String,
    pub recent_text: VecDeque<String>,
    pub open_points: Vec<OpenPoint>,
    pub questions: Vec<Question>,
    pub counter: u64,
    pub chunks_seen: u32,
    pub schema_version: u32,
}

impl Memory {
    pub fn new(session_id: &str) -> Self {
        Self { session_id: session_id.to_string(), schema_version: SCHEMA_VERSION, ..Self::default() }
    }

    pub fn next_id(&mut self, prefix: char) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    /// Whether the user supplied the goal (inferred goals use a stricter filter).
    pub fn goal_user_set(&self) -> bool {
        !self.goal.trim().is_empty() && !self.goal_inferred
    }

    /// Replace the summary; an empty reply keeps the old one.
    pub fn set_summary(&mut self, summary: &str) {
        let s = summary.trim();
        if !s.is_empty() {
            self.summary = s.chars().take(SUMMARY_MAX_CHARS).collect();
        }
    }

    pub fn push_recent(&mut self, clean_chunk: String) {
        self.recent_text.push_back(clean_chunk);
        while self.recent_text.len() > RECENT_CHUNKS {
            self.recent_text.pop_front();
        }
    }

    pub fn add_point(
        &mut self,
        text: &str,
        gap_type: GapType,
        speaker: Speaker,
        chunk_id: u32,
        source_ts_ms: i64,
    ) -> String {
        let id = self.next_id('P');
        self.open_points.push(OpenPoint {
            id: id.clone(),
            text: text.to_string(),
            gap_type,
            speaker,
            chunk_id,
            source_ts_ms,
        });
        if self.open_points.len() > MAX_OPEN_POINTS {
            self.open_points.remove(0);
        }
        id
    }

    pub fn resolve_points(&mut self, ids: &[String]) {
        self.open_points.retain(|p| !ids.contains(&p.id));
    }

    pub fn add_glossary(&mut self, entry: GlossaryEntry) {
        self.glossary.retain(|g| g.term != entry.term);
        self.glossary.push(entry);
        if self.glossary.len() > MAX_GLOSSARY {
            self.glossary.remove(0);
        }
    }

    pub fn question(&self, id: &str) -> Option<&Question> {
        self.questions.iter().find(|q| q.id == id)
    }

    pub fn open_count(&self) -> usize {
        self.questions.iter().filter(|q| q.state == QuestionState::Open).count()
    }

    /// Pending and open questions, shown to Stage 1 so it can mark them answered.
    pub fn live_questions(&self) -> Vec<&Question> {
        self.questions.iter().filter(|q| q.state.is_live()).collect()
    }

    /// The newest `DEDUP_WINDOW` question texts, any state.
    pub fn recent_question_texts(&self) -> Vec<&str> {
        let skip = self.questions.len().saturating_sub(DEDUP_WINDOW);
        self.questions.iter().skip(skip).map(|q| q.text.as_str()).collect()
    }

    /// Move a question to `to`; invalid transitions are rejected.
    pub fn transition(&mut self, id: &str, to: QuestionState, by_chunk: Option<u32>) -> Result<(), String> {
        let q = self.questions.iter_mut().find(|q| q.id == id).ok_or(format!("no question {id}"))?;
        if !q.state.can_become(to) {
            return Err(format!("{id}: {:?} -> {:?} not allowed", q.state, to));
        }
        q.state = to;
        if to == QuestionState::Resolved {
            q.resolved_by_chunk = by_chunk;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn question(id: &str, state: QuestionState) -> Question {
    Question {
        id: id.to_string(),
        text: format!("question {id}"),
        gap_type: GapType::NoNumber,
        source_ts_ms: 0,
        source_chunk: 0,
        state,
        created_at_ms: 0,
        ready_at_ms: 0,
        opened_at_ms: None,
        resolved_by_chunk: None,
        outcome_change: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_evict_oldest() {
        let mut m = Memory::new("s");
        for i in 0..20 {
            m.add_point(&format!("p{i}"), GapType::NoNumber, Speaker::Other, 1, 0);
            m.push_recent(format!("c{i}"));
        }
        assert_eq!(m.open_points.len(), MAX_OPEN_POINTS);
        assert_eq!(m.open_points[0].text, "p5");
        assert_eq!(m.recent_text.len(), RECENT_CHUNKS);
        m.set_summary(&"x".repeat(5_000));
        assert_eq!(m.summary.chars().count(), SUMMARY_MAX_CHARS);
        m.set_summary("  ");
        assert_eq!(m.summary.chars().count(), SUMMARY_MAX_CHARS);
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        let mut m = Memory::new("s");
        m.questions.push(question("Q1", QuestionState::Pending));
        assert!(m.transition("Q1", QuestionState::Resolved, Some(3)).is_err());
        assert!(m.transition("Q1", QuestionState::Open, None).is_ok());
        assert!(m.transition("Q1", QuestionState::Resolved, Some(3)).is_ok());
        assert_eq!(m.question("Q1").unwrap().resolved_by_chunk, Some(3));
        assert!(m.transition("Q1", QuestionState::Open, None).is_err());
    }

    #[test]
    fn serde_round_trip_is_identical() {
        let mut m = Memory::new("s1");
        m.goal = "decide".into();
        m.set_summary("summary");
        m.add_point("quote", GapType::FuzzyCriteria, Speaker::Presenter, 2, 500);
        m.questions.push(question("Q9", QuestionState::Open));
        m.add_glossary(GlossaryEntry { term: "Fivetran".into(), heard_as: vec!["5-tran".into()] });
        let json = serde_json::to_string(&m).unwrap();
        let back: Memory = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
    }
}
