//! Integration tests for the SQLite store, run against an in-memory database.

use chrono::{Duration, Utc};

use crate::storage::{
    NewParagraph, NewSegment, NewSession, ParagraphStore, SessionRepository, SessionStatus,
    SqliteStorage,
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
fn open_in_memory_migrates_schema() {
    let storage = store();
    assert_eq!(storage.schema_version().unwrap(), 3);
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

// --- Paragraph store (live-agent paragraph watcher) -------------------------

#[test]
fn create_and_list_paragraphs_in_order() {
    let storage = store();
    let session = storage.create_session(new_session("both", Utc::now())).unwrap();

    storage
        .create_paragraph(NewParagraph {
            session_id: session.id,
            seq: 1,
            prev_text: "".to_string(),
            new_text: "first chunk".to_string(),
            created_at: Utc::now(),
        })
        .unwrap();
    storage
        .create_paragraph(NewParagraph {
            session_id: session.id,
            seq: 2,
            prev_text: "first chunk".to_string(),
            new_text: "second chunk".to_string(),
            created_at: Utc::now(),
        })
        .unwrap();

    let paragraphs = storage.list_paragraphs(session.id).unwrap();
    assert_eq!(paragraphs.len(), 2);
    assert_eq!(paragraphs[0].seq, 1);
    assert_eq!(paragraphs[0].prev_text, "");
    assert_eq!(paragraphs[0].new_text, "first chunk");
    assert_eq!(paragraphs[1].seq, 2);
    assert_eq!(paragraphs[1].prev_text, "first chunk");
}

#[test]
fn paragraphs_are_per_session() {
    let storage = store();
    let a = storage.create_session(new_session("mic", Utc::now())).unwrap();
    let b = storage.create_session(new_session("mic", Utc::now())).unwrap();

    storage
        .create_paragraph(NewParagraph {
            session_id: a.id,
            seq: 1,
            prev_text: String::new(),
            new_text: "only in a".to_string(),
            created_at: Utc::now(),
        })
        .unwrap();

    assert_eq!(storage.list_paragraphs(a.id).unwrap().len(), 1);
    assert!(storage.list_paragraphs(b.id).unwrap().is_empty());
}

#[test]
fn delete_session_cascades_paragraphs() {
    let storage = store();
    let session = storage.create_session(new_session("mic", Utc::now())).unwrap();
    storage
        .create_paragraph(NewParagraph {
            session_id: session.id,
            seq: 1,
            prev_text: String::new(),
            new_text: "doomed".to_string(),
            created_at: Utc::now(),
        })
        .unwrap();

    storage.delete_session(session.id).unwrap();
    assert!(storage.list_paragraphs(session.id).unwrap().is_empty());
}
