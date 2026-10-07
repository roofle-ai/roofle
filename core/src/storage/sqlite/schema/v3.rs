//! V3 migration — link each audio segment to the transcript-log entry it was
//! folded into, so the UI can find a question's source segments by id.

/// The V3 delta: `audio_segments.log_seq` (the `transcript_log.sequence_no`
/// the segment belongs to; NULL while it sits in the still-open paragraph).
/// Existing rows are backfilled with the first log entry written at or after
/// the segment, which is the paragraph that absorbed it.
pub(super) const SCHEMA: &str = r#"
ALTER TABLE audio_segments ADD COLUMN log_seq INTEGER;

UPDATE audio_segments
SET log_seq = (
    SELECT t.sequence_no
    FROM transcript_log t
    WHERE t.session_id = audio_segments.session_id
      AND t.created_at >= audio_segments.created_at
    ORDER BY t.sequence_no
    LIMIT 1
);

CREATE INDEX IF NOT EXISTS idx_segments_session_log_seq
    ON audio_segments (session_id, log_seq);
"#;
