use live_agent::challenge::{agent_challenges, ChallengeContext};
use live_agent::notes::{log_step, logging_enabled};
use roofle_core::storage::{
    AudioSource, ParagraphStore, QuestionStatus, QuestionStore, TranscriptStore,
};
use tauri::{AppHandle, Emitter, Manager};

use super::client::configured_client;
use crate::state::{AppState, ChallengeUpdate, QuestionView};
use crate::{events, notify};

/// Caps keep every agent prompt bounded on long meetings; the newest text wins.
const MIC_CONTEXT_CHARS: usize = 4_000;
const EARLIER_TRANSCRIPT_CHARS: usize = 6_000;

/// Update the session's follow-up questions from one new paragraph, publish
/// the full list, and notify about new ones. Runs on a detached thread,
/// independently of the notes agent.
pub fn generate(app: AppHandle, session_id: i64, seq: i64, new_text: String) {
    std::thread::spawn(move || {
        let log = logging_enabled();
        let tag = format!("challenge #{seq} session={session_id}");
        let new_chars = new_text.chars().count();

        let client = match configured_client(&app) {
            Ok(client) => client,
            Err(reason) => {
                log_step(log, &tag, &format!("{reason}; skipping (new={new_chars} chars)"));
                return;
            }
        };
        log_step(
            log,
            &tag,
            &format!("agent start (model={}, new={new_chars} chars)", client.model()),
        );

        let storage = &app.state::<AppState>().storage;
        let open: Vec<(i64, String)> = storage
            .list_questions(session_id)
            .unwrap_or_default()
            .into_iter()
            .filter(|q| q.status == QuestionStatus::Open)
            .map(|q| (q.id, q.text))
            .collect();

        let summary = earlier_summary(storage, session_id, seq);
        let mic = mic_transcript(storage, session_id);
        let context = ChallengeContext { summary: &summary, mic: &mic };

        let res = match agent_challenges(&client, &new_text, &open, context, log) {
            Ok(res) => res,
            Err(e) => return log_step(log, &tag, &e),
        };
        log_step(
            log,
            &tag,
            &format!(
                "added={} answered={} stale={}",
                res.added.len(),
                res.answered.len(),
                res.stale.len()
            ),
        );

        for text in &res.added {
            let _ = storage.create_question(session_id, text, seq);
        }
        for (id, reason) in &res.answered {
            let _ = storage.resolve_question(*id, QuestionStatus::Answered, reason, Some(seq));
        }
        for (id, reason) in &res.stale {
            let _ = storage.resolve_question(*id, QuestionStatus::Stale, reason, None);
        }

        let questions = storage
            .list_questions(session_id)
            .unwrap_or_default()
            .iter()
            .map(QuestionView::from)
            .collect();
        let _ = app.emit(events::CHALLENGES, ChallengeUpdate { seq, questions });
        notify::notify_new_questions(&app, &res.added);
    });
}

/// What was discussed *before* paragraph `seq`: the live notes (when they
/// predate it) plus the tail of the earlier transcript. Notes are written by a
/// concurrent agent, so a note at or past `seq` already contains the newest
/// chunk and is left out — otherwise its content would look "already covered".
fn earlier_summary<S: QuestionStore + TranscriptStore>(
    storage: &S,
    session_id: i64,
    seq: i64,
) -> String {
    let mut parts = Vec::new();

    if let Ok(Some(note)) = storage.latest_note(session_id) {
        if note.sequence_no < seq && !note.text.trim().is_empty() {
            parts.push(format!("Notes so far:\n{}", note.text.trim()));
        }
    }

    let earlier: Vec<String> = storage
        .transcript_after(session_id, 0)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.sequence_no < seq)
        .map(|e| e.text)
        .collect();
    if !earlier.is_empty() {
        let tail = tail_chars(&earlier.join("\n\n"), EARLIER_TRANSCRIPT_CHARS);
        parts.push(format!("Recent earlier transcript:\n{tail}"));
    }

    parts.join("\n\n")
}

/// Everything the user has said out loud so far, newest text kept when capped.
fn mic_transcript<S: ParagraphStore>(storage: &S, session_id: i64) -> String {
    let texts = storage
        .audio_texts(session_id, AudioSource::Mic)
        .unwrap_or_default();
    tail_chars(&texts.join(" "), MIC_CONTEXT_CHARS)
}

/// The last `max` chars of `s`, cut at a char boundary.
fn tail_chars(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    s.chars().skip(count - max).collect()
}

#[cfg(test)]
mod tests {
    use super::tail_chars;

    #[test]
    fn tail_keeps_the_newest_chars() {
        assert_eq!(tail_chars("abcdef", 3), "def");
        assert_eq!(tail_chars("abc", 10), "abc");
        assert_eq!(tail_chars("héllo wörld", 5), "wörld");
    }
}
