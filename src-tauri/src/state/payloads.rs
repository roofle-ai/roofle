use roofle_core::storage::{AudioSource, Question, QuestionStatus};

/// One finalized utterance, offset in seconds from recording start.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TranscriptCue {
    pub start_secs: f64,
    pub source: AudioSource,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NotesUpdate {
    pub seq: i64,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QuestionView {
    pub id: i64,
    pub text: String,
    /// `open`, `answered`, or `stale`.
    pub status: &'static str,
    /// Agent-written resolution text: the answer when `answered`, the reason when `stale`.
    pub reason: Option<String>,
}

impl From<&Question> for QuestionView {
    fn from(q: &Question) -> Self {
        Self {
            id: q.id,
            text: q.text.clone(),
            status: match q.status {
                QuestionStatus::Open => "open",
                QuestionStatus::Answered => "answered",
                QuestionStatus::Stale => "stale",
            },
            reason: q.resolution_reason.clone(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChallengeUpdate {
    pub seq: i64,
    pub questions: Vec<QuestionView>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TitleUpdate {
    pub session_id: i64,
    pub title: String,
}
