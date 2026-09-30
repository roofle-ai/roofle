//! Integration tests for the SQLite store, run against an in-memory database.

use chrono::{Duration, Utc};

use crate::storage::{
    AudioSource, NewAudioSegment, NewSegment, NewSession, ParagraphStore, QuestionStore,
    SessionRepository, SessionStatus, SqliteStorage, TranscriptStore,
};

fn store() -> SqliteStorage {
    SqliteStorage::open_in_memory().unwrap()
}

fn new_session(source: &str, started: chrono::DateTime<Utc>) -> NewSession {
    NewSession {
        source: source.to_string(),
        model_id: "test-model".to_string(),
        started_at: started,
    }
}

#[test]
fn open_in_memory_initializes_current_schema() {
    let storage = store();
    // The current schema is V2 (adds `session_analysis`).
    assert_eq!(storage.schema_version().unwrap(), 2);
}

#[test]
fn save_and_load_analysis_round_trip() {
    let storage = store();
    let session = storage
        .create_session(new_session("mic", Utc::now()))
        .unwrap();

    // No analysis yet.
    assert!(storage.load_analysis(session.id).unwrap().is_none());

    // Persist then reload; the stored row keeps context + result JSON.
    storage
        .save_analysis(session.id, "Selling a product", r#"{"summary":"ok"}"#)
        .unwrap();
    let stored = storage.load_analysis(session.id).unwrap().unwrap();
    assert_eq!(stored.context, "Selling a product");
    assert!(stored.result_json.contains("summary"));

    // Re-saving overwrites (one row per session).
    storage
        .save_analysis(session.id, "Buying a product", r#"{"summary":"buyer"}"#)
        .unwrap();
    let stored = storage.load_analysis(session.id).unwrap().unwrap();
    assert_eq!(stored.context, "Buying a product");
    assert!(stored.result_json.contains("buyer"));
}

#[test]
fn save_and_load_notes_round_trip() {
    let storage = store();
    let session = storage
        .create_session(new_session("mic", Utc::now()))
        .unwrap();

    // Default notes are empty.
    let detail = storage.get_session(session.id).unwrap().unwrap();
    assert_eq!(detail.notes, "");

    // Persist an editable notes document, then read it back via get_session.
    storage.save_notes(session.id, "Decision: ship v2 on Friday.\nOwner: Alex.").unwrap();
    let detail = storage.get_session(session.id).unwrap().unwrap();
    assert_eq!(detail.notes, "Decision: ship v2 on Friday.\nOwner: Alex.");

    // Overwriting replaces the whole document.
    storage.save_notes(session.id, "Revised notes after review.").unwrap();
    let detail = storage.get_session(session.id).unwrap().unwrap();
    assert_eq!(detail.notes, "Revised notes after review.");

    // Saving to a missing session is an error.
    assert!(storage.save_notes(999, "nope").is_err());
}

#[test]
fn set_title_round_trip() {
    let storage = store();
    let session = storage
        .create_session(new_session("mic", Utc::now()))
        .unwrap();

    // Default title is empty.
    let detail = storage.get_session(session.id).unwrap().unwrap();
    assert_eq!(detail.session.title, "");

    // The offline title agent renames the session; read it back via get_session.
    storage.set_title(session.id, "Q3 Roadmap Planning").unwrap();
    let detail = storage.get_session(session.id).unwrap().unwrap();
    assert_eq!(detail.session.title, "Q3 Roadmap Planning");

    // Setting a title on a missing session is an error.
    assert!(storage.set_title(999, "nope").is_err());
}

#[test]
fn create_and_list_session() {
    let storage = store();
    let started = Utc::now();
    let session = storage.create_session(new_session("mic", started)).unwrap();

    assert!(session.id > 0);
    assert_eq!(session.status, SessionStatus::Recording);

    let list = storage.list_sessions(None).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, session.id);
    assert_eq!(list[0].source, "mic");
    assert_eq!(list[0].started_at, started);
    assert_eq!(list[0].ended_at, None);
}

#[test]
fn append_segments_and_get_session_in_order() {
    let storage = store();
    let session = storage.create_session(new_session("both", Utc::now())).unwrap();

    storage
        .append_segments(&[
            NewSegment {
                session_id: session.id,
                seq: 1,
                start_secs: 0.5,
                text: "First".to_string(),
                created_at: Utc::now(),
            },
            NewSegment {
                session_id: session.id,
                seq: 2,
                start_secs: 4.0,
                text: "Second sentence.".to_string(),
                created_at: Utc::now(),
            },
        ])
        .unwrap();

    let detail = storage.get_session(session.id).unwrap().unwrap();
    assert_eq!(detail.segments.len(), 2);
    assert_eq!(detail.segments[0].text, "First");
    assert_eq!(detail.segments[1].seq, 2);
    assert_eq!(detail.transcript, "First Second sentence.");
}

#[test]
fn finalize_session_sets_end_and_status() {
    let storage = store();
    let session = storage.create_session(new_session("system", Utc::now())).unwrap();

    let ended = Utc::now() + Duration::seconds(90);
    storage.finalize_session(session.id, ended, 90.5).unwrap();

    let list = storage.list_sessions(None).unwrap();
    assert_eq!(list[0].status, SessionStatus::Completed);
    assert_eq!(list[0].ended_at, Some(ended));
    assert_eq!(list[0].duration_secs, 90.5);
}

#[test]
fn delete_session_cascades_segments() {
    let storage = store();
    let session = storage.create_session(new_session("mic", Utc::now())).unwrap();
    storage
        .append_segments(&[NewSegment {
            session_id: session.id,
            seq: 1,
            start_secs: 0.0,
            text: "Doomed".to_string(),
            created_at: Utc::now(),
        }])
        .unwrap();

    storage.delete_session(session.id).unwrap();

    assert!(storage.get_session(session.id).unwrap().is_none());
    assert!(storage.list_sessions(None).unwrap().is_empty());
}

#[test]
fn missing_session_is_not_found() {
    let storage = store();
    assert!(storage.get_session(99).unwrap().is_none());
    assert!(matches!(
        storage.finalize_session(99, Utc::now(), 1.0),
        Err(crate::storage::StorageError::NotFound(99))
    ));
    assert!(matches!(
        storage.delete_session(99),
        Err(crate::storage::StorageError::NotFound(99))
    ));
}

#[test]
fn list_sessions_newest_first_with_limit() {
    let storage = store();
    let a = storage.create_session(new_session("mic", Utc::now())).unwrap();
    let b = storage
        .create_session(new_session("mic", Utc::now() + Duration::minutes(1)))
        .unwrap();

    let all = storage.list_sessions(None).unwrap();
    assert_eq!(all[0].id, b.id);

    let limited = storage.list_sessions(Some(1)).unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].id, b.id);
    let _ = a;
}

#[test]
fn continue_session_reopens_and_accumulates() {
    let storage = store();
    let session = storage.create_session(new_session("mic", Utc::now())).unwrap();
    storage
        .append_segments(&[NewSegment {
            session_id: session.id,
            seq: 1,
            start_secs: 0.0,
            text: "hello".to_string(),
            created_at: Utc::now(),
        }])
        .unwrap();
    storage.finalize_session(session.id, Utc::now(), 30.0).unwrap();

    // Continuing returns the accumulated duration as the timestamp offset.
    let offset = storage.continue_session(session.id).unwrap();
    assert_eq!(offset, 30.0);

    // The reopened session stays recording and segments continue the sequence.
    storage
        .append_segments(&[NewSegment {
            session_id: session.id,
            seq: 2,
            start_secs: 31.0,
            text: "again".to_string(),
            created_at: Utc::now(),
        }])
        .unwrap();
    assert_eq!(storage.last_seq(session.id).unwrap(), 2);

    let list = storage.list_sessions(None).unwrap();
    assert_eq!(list[0].status, SessionStatus::Recording);
    // continue_session bumped updated_at past the original start.
    assert!(list[0].updated_at >= list[0].started_at);

    // Finalizing again accumulates the total duration.
    storage.finalize_session(session.id, Utc::now(), 10.0).unwrap();
    let list = storage.list_sessions(None).unwrap();
    assert_eq!(list[0].duration_secs, 40.0);
    assert_eq!(list[0].status, SessionStatus::Completed);
}

#[test]
fn list_preview_shows_first_words() {
    let storage = store();
    let session = storage.create_session(new_session("mic", Utc::now())).unwrap();
    storage
        .append_segments(&[NewSegment {
            session_id: session.id,
            seq: 1,
            start_secs: 0.0,
            text: "one two three four five six seven eight nine ten eleven".to_string(),
            created_at: Utc::now(),
        }])
        .unwrap();

    let list = storage.list_sessions(None).unwrap();
    assert_eq!(list[0].preview, "one two three four five six seven eight nine ten");
}

// --- Paragraph + transcript store (db-persist model) -----------------------

fn new_segment(session_id: i64, text: &str, start_ms: i64, end_ms: i64) -> NewAudioSegment {
    NewAudioSegment {
        session_id,
        source: AudioSource::System,
        text: text.to_string(),
        start_ms,
        end_ms,
        confidence: None,
        created_at: Utc::now(),
    }
}

#[test]
fn add_segment_folds_into_single_paragraph() {
    let storage = store();
    let session = storage.create_session(new_session("both", Utc::now())).unwrap();

    storage.add_segment(&new_segment(session.id, "first", 0, 500)).unwrap();
    storage.add_segment(&new_segment(session.id, "second", 600, 1000)).unwrap();

    // Both segments accumulate into the session's one paragraph row.
    let (_, text) = storage.finalize_paragraph(session.id).unwrap().unwrap();
    assert_eq!(text, "first second");
}

#[test]
fn audio_texts_returns_one_source_in_time_order() {
    let storage = store();
    let session = storage.create_session(new_session("both", Utc::now())).unwrap();

    let mut mic_late = new_segment(session.id, "mic later", 900, 950);
    mic_late.source = AudioSource::Mic;
    let mut mic_early = new_segment(session.id, "mic early", 100, 150);
    mic_early.source = AudioSource::Mic;
    storage.add_segment(&mic_early).unwrap();
    storage.add_segment(&new_segment(session.id, "them", 500, 600)).unwrap();
    storage.add_segment(&mic_late).unwrap();

    let mic = storage.audio_texts(session.id, AudioSource::Mic).unwrap();
    assert_eq!(mic, vec!["mic early".to_string(), "mic later".to_string()]);
    let system = storage.audio_texts(session.id, AudioSource::System).unwrap();
    assert_eq!(system, vec!["them".to_string()]);
}

#[test]
fn finalize_paragraph_moves_text_to_log_and_resets() {
    let storage = store();
    let session = storage.create_session(new_session("system", Utc::now())).unwrap();

    storage.add_segment(&new_segment(session.id, "hello world", 0, 100)).unwrap();
    let (seq, text) = storage.finalize_paragraph(session.id).unwrap().unwrap();
    assert_eq!(seq, 1);
    assert_eq!(text, "hello world");

    // Finalizing an empty paragraph is a no-op.
    assert!(storage.finalize_paragraph(session.id).unwrap().is_none());

    // New segments start a fresh paragraph and continue the sequence.
    storage.add_segment(&new_segment(session.id, "again", 200, 300)).unwrap();
    let (seq, text) = storage.finalize_paragraph(session.id).unwrap().unwrap();
    assert_eq!(seq, 2);
    assert_eq!(text, "again");
    assert_eq!(storage.last_transcript_sequence(session.id).unwrap(), 2);

    let after = storage.transcript_after(session.id, 1).unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].sequence_no, 2);
}

#[test]
fn append_transcript_assigns_monotonic_sequence() {
    let storage = store();
    let session = storage.create_session(new_session("system", Utc::now())).unwrap();

    let seq1 = storage.append_transcript(session.id, AudioSource::System, "one", 0).unwrap();
    let seq2 = storage.append_transcript(session.id, AudioSource::System, "two", 10).unwrap();
    assert_eq!(seq1, 1);
    assert_eq!(seq2, 2);
    assert_eq!(storage.last_transcript_sequence(session.id).unwrap(), 2);
    assert!(storage.newest_transcript(session.id).unwrap().is_some());
}

#[test]
fn delete_session_cascades_paragraphs_and_transcript() {
    let storage = store();
    let session = storage.create_session(new_session("mic", Utc::now())).unwrap();
    storage.add_segment(&new_segment(session.id, "doomed", 0, 10)).unwrap();

    storage.delete_session(session.id).unwrap();
    assert!(storage.newest_transcript(session.id).unwrap().is_none());
}

// --- Rolling summary + questions (db-persist model) ------------------------

#[test]
fn rolling_summary_round_trips() {
    let storage = store();
    let session = storage.create_session(new_session("system", Utc::now())).unwrap();
    assert!(storage.load_rolling_summary(session.id).unwrap().is_none());

    storage.save_rolling_summary(session.id, "notes so far", 5).unwrap();
    let s = storage.load_rolling_summary(session.id).unwrap().unwrap();
    assert_eq!(s.summary_text, "notes so far");
    assert_eq!(s.last_sequence_no, 5);

    // Overwriting replaces the summary.
    storage.save_rolling_summary(session.id, "updated notes", 7).unwrap();
    let s = storage.load_rolling_summary(session.id).unwrap().unwrap();
    assert_eq!(s.summary_text, "updated notes");
    assert_eq!(s.last_sequence_no, 7);
}

#[test]
fn live_notes_override_and_latest() {
    let storage = store();
    let session = storage.create_session(new_session("system", Utc::now())).unwrap();
    assert!(storage.latest_note(session.id).unwrap().is_none());

    storage.save_note(session.id, 1, "first").unwrap();
    storage.save_note(session.id, 2, "second").unwrap();

    let latest = storage.latest_note(session.id).unwrap().unwrap();
    assert_eq!(latest.text, "second");
    assert_eq!(latest.sequence_no, 2);
}

#[test]
fn questions_round_trip_and_stale_transition() {
    let storage = store();
    let session = storage.create_session(new_session("system", Utc::now())).unwrap();
    storage.create_question(session.id, "what next?", 3).unwrap();
    storage.create_question(session.id, "who owns it?", 4).unwrap();

    let open = storage.list_questions(session.id).unwrap();
    assert_eq!(open.len(), 2);
    assert!(open.iter().all(|q| q.status == crate::storage::QuestionStatus::Open));

    storage.mark_open_questions_stale(session.id).unwrap();
    let stale = storage.list_questions(session.id).unwrap();
    assert!(stale.iter().all(|q| q.status == crate::storage::QuestionStatus::Stale));
}
