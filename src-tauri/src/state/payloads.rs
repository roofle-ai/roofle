use roofle_core::storage::{
    AudioSource, Question, QuestionStatus, QuestionStore, StorageError, TranscriptEntry,
    TranscriptStore,
};

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

/// The transcript entry a question was raised from, so the UI can show it came
/// from the conversation rather than from the model's own invention.
#[derive(Debug, Clone, serde::Serialize)]
pub struct QuestionSource {
    pub seq: i64,
    /// `you` (microphone) or `them` (system audio).
    pub speaker: &'static str,
    pub text: String,
    pub start_ms: i64,
}

impl QuestionSource {
    fn new(e: &TranscriptEntry) -> Self {
        Self {
            seq: e.sequence_no,
            speaker: match e.source {
                AudioSource::Mic => "you",
                AudioSource::System => "them",
            },
            text: e.text.trim().to_string(),
            start_ms: e.start_ms,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QuestionView {
    pub id: i64,
    pub text: String,
    /// `open`, `answered`, or `stale`.
    pub status: &'static str,
    /// Agent-written resolution text: the answer when `answered`, the reason when `stale`.
    pub reason: Option<String>,
    /// Where in the transcript it came from; `None` if the entry is gone.
    pub source: Option<QuestionSource>,
}

/// A session's questions, each linked to its source transcript entry.
pub fn question_views<S: QuestionStore + TranscriptStore>(
    storage: &S,
    session_id: i64,
) -> Result<Vec<QuestionView>, StorageError> {
    let questions = storage.list_questions(session_id)?;
    let transcript = storage.transcript_after(session_id, 0)?;
    Ok(questions
        .iter()
        .map(|q| {
            let mut view = QuestionView::from(q);
            view.source = transcript
                .iter()
                .find(|e| e.sequence_no == q.source_sequence_no)
                .map(QuestionSource::new);
            view
        })
        .collect())
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
            source: None,
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
