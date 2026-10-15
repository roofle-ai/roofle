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
    pub session_id: i64,
    pub seq: i64,
    pub text: String,
}

/// Emitted once the end-of-call notes pass is over. `ready` is false when the
/// notes could not be brought up to date (e.g. AI not configured).
#[derive(Debug, Clone, serde::Serialize)]
pub struct WrapUp {
    pub session_id: i64,
    pub ready: bool,
    pub action_items: usize,
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
    /// Where in the transcript it was answered; `None` unless `answered` by a later line.
    pub answer_source: Option<QuestionSource>,
    /// The user's feedback (`asked` or `dismissed`), if any.
    pub feedback: Option<String>,
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
            view.answer_source = q.resolved_sequence_no.and_then(|n| {
                transcript.iter().find(|e| e.sequence_no == n).map(QuestionSource::new)
            });
            // Pipeline questions know the exact line's time, not just the paragraph's.
            if let (Some(ts), Some(source)) = (q.source_ts_ms, view.source.as_mut()) {
                source.start_ms = ts;
            }
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
            answer_source: None,
            feedback: q.feedback.clone(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChallengeUpdate {
    pub session_id: i64,
    pub seq: i64,
    pub questions: Vec<QuestionView>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TitleUpdate {
    pub session_id: i64,
    pub title: String,
}
