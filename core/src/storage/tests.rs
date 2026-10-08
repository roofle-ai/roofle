//! Integration tests for the SQLite store, run against an in-memory database.

use chrono::{Duration, Utc};

use crate::storage::{
    AudioSource, NewAudioSegment, NewSession, ParagraphStore, QuestionStore, SessionRepository,
    SessionStatus, SqliteStorage, TranscriptStore,
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
    // The current schema is V5 (question pipeline tables).
    assert_eq!(storage.schema_version().unwrap(), 5);
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
fn add_segments_and_get_session_in_order() {
    let storage = store();
    let session = storage.create_session(new_session("both", Utc::now())).unwrap();

    storage.add_segment(&new_segment(session.id, "First", 0, 500)).unwrap();
    storage.add_segment(&new_segment(session.id, "Second sentence.", 4000, 5000)).unwrap();

    let detail = storage.get_session(session.id).unwrap().unwrap();
    assert_eq!(detail.segments.len(), 2);
    assert_eq!(detail.segments[0].text, "First");
    // The single audio_segments store keeps the source tag on each utterance.
    assert_eq!(detail.segments[0].source, AudioSource::System);
    assert_eq!(detail.segments[1].text, "Second sentence.");
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
fn delete_session_cascades_audio_segments() {
    let storage = store();
    let session = storage.create_session(new_session("mic", Utc::now())).unwrap();
    storage.add_segment(&new_segment(session.id, "Doomed", 0, 10)).unwrap();

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
    storage.add_segment(&new_segment(session.id, "hello", 0, 100)).unwrap();
    storage.finalize_session(session.id, Utc::now(), 30.0).unwrap();

    // Continuing returns the accumulated duration as the timestamp offset.
    let offset = storage.continue_session(session.id).unwrap();
    assert_eq!(offset, 30.0);

    // The reopened session stays recording and audio segments keep appending.
    storage.add_segment(&new_segment(session.id, "again", 31000, 32000)).unwrap();
    let detail = storage.get_session(session.id).unwrap().unwrap();
    assert_eq!(detail.segments.len(), 2);
    assert_eq!(detail.segments[1].text, "again");

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
        .add_segment(&new_segment(
            session.id,
            "one two three four five six seven eight nine ten eleven",
            0,
            100,
        ))
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
fn finalize_links_segments_to_their_log_entry() {
    let storage = store();
    let session = storage.create_session(new_session("both", Utc::now())).unwrap();

    storage.add_segment(&new_segment(session.id, "one", 0, 100)).unwrap();
    storage.add_segment(&new_segment(session.id, "two", 100, 200)).unwrap();
    storage.finalize_paragraph(session.id).unwrap();
    storage.add_segment(&new_segment(session.id, "three", 300, 400)).unwrap();
    storage.finalize_paragraph(session.id).unwrap();
    storage.add_segment(&new_segment(session.id, "pending", 500, 600)).unwrap();

    let detail = storage.get_session(session.id).unwrap().unwrap();
    let links: Vec<_> = detail.segments.iter().map(|s| s.log_seq).collect();
    assert_eq!(links, vec![Some(1), Some(1), Some(2), None]);
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
    // Each entry keeps its own first segment's start, not the reset zero.
    let entries = storage.transcript_after(session.id, 0).unwrap();
    assert_eq!(entries[0].start_ms, 0);
    assert_eq!(entries[1].start_ms, 200);
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
    for (key, text, seq) in [("Q1", "what next?", 3), ("Q2", "who owns it?", 4)] {
        storage
            .create_pipeline_question(&crate::storage::NewPipelineQuestion {
                session_id: session.id, key, text, gap_type: "NO_NUMBER",
                source_sequence_no: seq, source_ts_ms: 0,
            })
            .unwrap();
    }

    let open = storage.list_questions(session.id).unwrap();
    assert_eq!(open.len(), 2);
    assert!(open.iter().all(|q| q.status == crate::storage::QuestionStatus::Open));

    for key in ["Q1", "Q2"] {
        storage
            .resolve_question_by_key(session.id, key, crate::storage::QuestionStatus::Stale, "old", None)
            .unwrap();
    }
    let stale = storage.list_questions(session.id).unwrap();
    assert!(stale.iter().all(|q| q.status == crate::storage::QuestionStatus::Stale));
}

#[test]
fn pipeline_questions_memory_and_runs_round_trip() {
    use crate::storage::{NewPipelineQuestion, PipelineRun};
    let storage = store();
    let session = storage.create_session(new_session("system", Utc::now())).unwrap();
    let q = NewPipelineQuestion {
        session_id: session.id,
        key: "Q7",
        text: "What happens if the trigger fails?",
        gap_type: "PLAN_NO_FALLBACK",
        source_sequence_no: 2,
        source_ts_ms: 42_000,
    };
    storage.create_pipeline_question(&q).unwrap();
    storage.set_question_feedback(session.id, "Q7", "asked").unwrap();
    let listed = storage.list_questions(session.id).unwrap();
    assert_eq!(listed[0].question_key.as_deref(), Some("Q7"));
    assert_eq!(listed[0].source_ts_ms, Some(42_000));
    assert_eq!(listed[0].feedback.as_deref(), Some("asked"));

    storage
        .resolve_question_by_key(session.id, "Q7", crate::storage::QuestionStatus::Answered, "later", Some(3))
        .unwrap();
    assert_eq!(storage.list_questions(session.id).unwrap()[0].status, crate::storage::QuestionStatus::Answered);

    assert_eq!(storage.load_memory(session.id).unwrap(), None);
    storage.save_memory(session.id, "{\"a\":1}", 1).unwrap();
    storage.save_memory(session.id, "{\"a\":2}", 1).unwrap();
    assert_eq!(storage.load_memory(session.id).unwrap().as_deref(), Some("{\"a\":2}"));

    storage
        .log_pipeline_run(&PipelineRun {
            session_id: session.id, chunk_id: 2, stage: "judge", model: "m",
            prompt_version: "judge.v1", input: "", output: Some("{}"), error: None,
        })
        .unwrap();
}
