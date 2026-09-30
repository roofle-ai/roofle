//! Every SQL string in the store, in one place — query constants only, named by
//! table + verb. The schema lives in the `schema` module (applied by
//! `schema::initialize`), so this file is purely about reads and writes.

// --- sessions -------------------------------------------------------------

/// Insert a session row; the id comes back via `last_insert_rowid`.
pub(super) const INSERT_SESSION: &str = r#"
INSERT INTO sessions (title, source, model_id, started_at, status, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6)
"#;

/// Close a session: set `ended_at`, accumulate duration, mark completed.
pub(super) const UPDATE_SESSION_FINALIZE: &str = r#"
UPDATE sessions
SET ended_at = ?1, duration_secs = duration_secs + ?2,
    status = 'completed', updated_at = ?1
WHERE id = ?3
"#;

/// Read a session's accumulated duration (for the continue offset).
pub(super) const SELECT_SESSION_DURATION: &str =
    "SELECT duration_secs FROM sessions WHERE id = ?1";

/// Reopen a stored session for continued recording.
pub(super) const UPDATE_SESSION_RESUME: &str =
    "UPDATE sessions SET status = 'recording', updated_at = ?1 WHERE id = ?2";

/// Sessions by most recent activity, with a first-words preview.
pub(super) const SELECT_SESSIONS: &str = r#"
SELECT s.id, s.title, s.source, s.model_id, s.started_at, s.ended_at,
       s.duration_secs, s.status, s.updated_at,
       (SELECT text FROM segments WHERE session_id = s.id ORDER BY seq LIMIT 1)
FROM sessions s
ORDER BY s.updated_at DESC
LIMIT ?1
"#;

/// One session row by id (includes the editable notes document).
pub(super) const SELECT_SESSION: &str = r#"
SELECT id, title, source, model_id, started_at, ended_at, duration_secs, status, updated_at, notes
FROM sessions
WHERE id = ?1
"#;

/// Persist the editable notes document for a session.
pub(super) const UPDATE_SESSION_NOTES: &str =
    "UPDATE sessions SET notes = ?1 WHERE id = ?2";

/// Rename a session (the offline title-generator agent).
pub(super) const UPDATE_SESSION_TITLE: &str =
    "UPDATE sessions SET title = ?1 WHERE id = ?2";

/// Delete a session; children cascade.
pub(super) const DELETE_SESSION: &str = "DELETE FROM sessions WHERE id = ?1";

// --- segments -------------------------------------------------------------

/// Append one finalized utterance.
pub(super) const INSERT_SEGMENT: &str = r#"
INSERT INTO segments (session_id, seq, start_secs, text, created_at)
VALUES (?1, ?2, ?3, ?4, ?5)
"#;

/// Highest segment sequence of a session (0 when it has none).
pub(super) const SELECT_LAST_SEG_SEQ: &str =
    "SELECT COALESCE(MAX(seq), 0) FROM segments WHERE session_id = ?1";

/// A session's segments in recognition order.
pub(super) const SELECT_SEGMENTS: &str = r#"
SELECT id, session_id, seq, start_secs, text, created_at
FROM segments
WHERE session_id = ?1
ORDER BY seq
"#;

// --- audio_segments -------------------------------------------------------

/// Insert one source-tagged audio segment.
pub(super) const INSERT_AUDIO_SEGMENT: &str = r#"
INSERT INTO audio_segments
    (session_id, source, text, start_ms, end_ms, confidence, created_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
"#;

/// A session's audio-segment texts from one source, oldest first.
pub(super) const SELECT_AUDIO_TEXTS: &str = r#"
SELECT text
FROM audio_segments
WHERE session_id = ?1 AND source = ?2
ORDER BY start_ms, id
"#;

// --- paragraphs -----------------------------------------------------------

/// The session's current paragraph row.
pub(super) const SELECT_PARAGRAPH: &str = r#"
SELECT session_id, source, text, start_ms, end_ms, sequence_no, created_at, updated_at
FROM paragraphs
WHERE session_id = ?1
"#;

/// Start the session's paragraph row (first fold).
pub(super) const INSERT_PARAGRAPH: &str = r#"
INSERT INTO paragraphs
    (session_id, source, text, start_ms, end_ms, sequence_no, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)
"#;

/// Fold a segment into the paragraph: append text and extend end_ms.
/// The separator space is skipped when the paragraph was just reset (empty).
pub(super) const UPDATE_PARAGRAPH_APPEND: &str = r#"
UPDATE paragraphs
SET text = CASE WHEN text = '' THEN ?1 ELSE text || ' ' || ?1 END,
    end_ms = ?2, updated_at = ?3
WHERE session_id = ?4
"#;

/// Reset the paragraph row for the next paragraph, recording the last
/// transcript sequence folded.
pub(super) const RESET_PARAGRAPH: &str = r#"
UPDATE paragraphs
SET text = '', start_ms = 0, end_ms = 0, sequence_no = ?1, updated_at = ?2
WHERE session_id = ?3
"#;

// --- transcript_log -------------------------------------------------------

/// Highest merged-log sequence of a session (0 when it has none).
pub(super) const SELECT_LAST_TRANSCRIPT_SEQ: &str =
    "SELECT COALESCE(MAX(sequence_no), 0) FROM transcript_log WHERE session_id = ?1";

/// Append a finalized paragraph to the merged log.
pub(super) const INSERT_TRANSCRIPT: &str = r#"
INSERT INTO transcript_log
    (session_id, source, text, start_ms, sequence_no, created_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6)
"#;

/// The newest (highest-sequence) transcript entry of a session.
pub(super) const SELECT_NEWEST_TRANSCRIPT: &str = r#"
SELECT id, session_id, source, text, start_ms, sequence_no, created_at
FROM transcript_log
WHERE session_id = ?1
ORDER BY sequence_no DESC
LIMIT 1
"#;

/// A session's transcript entries after a given sequence, in order.
pub(super) const SELECT_TRANSCRIPT_AFTER: &str = r#"
SELECT id, session_id, source, text, start_ms, sequence_no, created_at
FROM transcript_log
WHERE session_id = ?1 AND sequence_no > ?2
ORDER BY sequence_no
"#;

// --- rolling_summaries ----------------------------------------------------

/// Overwrite a session's rolling summary (upsert).
pub(super) const UPSERT_ROLLING_SUMMARY: &str = r#"
INSERT INTO rolling_summaries (session_id, summary_text, last_sequence_no, updated_at)
VALUES (?1, ?2, ?3, ?4)
ON CONFLICT(session_id) DO UPDATE SET
    summary_text = excluded.summary_text,
    last_sequence_no = excluded.last_sequence_no,
    updated_at = excluded.updated_at
"#;

/// A session's rolling summary, if any.
pub(super) const SELECT_ROLLING_SUMMARY: &str = r#"
SELECT session_id, summary_text, last_sequence_no, updated_at
FROM rolling_summaries
WHERE session_id = ?1
"#;

// --- questions ------------------------------------------------------------

/// A session's questions, newest first.
pub(super) const SELECT_QUESTIONS: &str = r#"
SELECT id, session_id, text, lens, status, resolution_reason,
       source_sequence_no, resolved_sequence_no, created_at, updated_at
FROM questions
WHERE session_id = ?1
ORDER BY id DESC
"#;

/// Insert one open question.
pub(super) const INSERT_QUESTION: &str = r#"
INSERT INTO questions
    (session_id, text, lens, status, resolution_reason,
     source_sequence_no, resolved_sequence_no, created_at, updated_at)
VALUES (?1, ?2, ?3, 'open', NULL, ?4, NULL, ?5, ?5)
"#;

/// Mark every currently-open question of a session stale.
pub(super) const UPDATE_QUESTIONS_STALE: &str = r#"
UPDATE questions
SET status = 'stale', updated_at = ?1
WHERE session_id = ?2 AND status = 'open'
"#;

/// Resolve a single question: set status, reason, and resolving sequence.
pub(super) const UPDATE_QUESTION_RESOLVE: &str = r#"
UPDATE questions
SET status = ?1, resolution_reason = ?2, resolved_sequence_no = ?3, updated_at = ?4
WHERE id = ?5
"#;

// --- live_notes ------------------------------------------------------------

/// Overwrite the session's live note (upsert).
pub(super) const UPSERT_LIVE_NOTE: &str = r#"
INSERT INTO live_notes (session_id, sequence_no, text, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?4)
ON CONFLICT(session_id) DO UPDATE SET
    sequence_no = excluded.sequence_no,
    text = excluded.text,
    updated_at = excluded.updated_at
"#;

/// The session's live note (the running notes doc).
pub(super) const SELECT_LATEST_LIVE_NOTE: &str = r#"
SELECT session_id, sequence_no, text, created_at, updated_at
FROM live_notes
WHERE session_id = ?1
"#;

// --- session_analysis (offline self-assessment) -----------------------------

/// Upsert a session's offline analysis (one row per session).
pub(super) const UPSERT_SESSION_ANALYSIS: &str = r#"
INSERT INTO session_analysis (session_id, context, result_json, created_at, updated_at)
VALUES (?1, ?2, ?3, ?4, ?4)
ON CONFLICT(session_id) DO UPDATE SET
    context = excluded.context,
    result_json = excluded.result_json,
    updated_at = excluded.updated_at
"#;

/// A session's persisted offline analysis, if any.
pub(super) const SELECT_SESSION_ANALYSIS: &str = r#"
SELECT context, result_json
FROM session_analysis
WHERE session_id = ?1
"#;
