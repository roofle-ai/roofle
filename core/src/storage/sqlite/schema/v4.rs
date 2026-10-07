//! V4 migration — repair `transcript_log` start times and sources.
//!
//! Before this, a paragraph reset zeroed its `start_ms` and never took the next
//! first segment's start or source, so every entry after a session's first
//! carried `start_ms = 0`. Entries are recomputed from the segments linked to
//! them (V3 `log_seq`): the earliest segment's start and source.

pub(super) const SCHEMA: &str = r#"
UPDATE transcript_log
SET start_ms = (
        SELECT a.start_ms FROM audio_segments a
        WHERE a.session_id = transcript_log.session_id AND a.log_seq = transcript_log.sequence_no
        ORDER BY a.id LIMIT 1),
    source = (
        SELECT a.source FROM audio_segments a
        WHERE a.session_id = transcript_log.session_id AND a.log_seq = transcript_log.sequence_no
        ORDER BY a.id LIMIT 1)
WHERE EXISTS (
    SELECT 1 FROM audio_segments a
    WHERE a.session_id = transcript_log.session_id AND a.log_seq = transcript_log.sequence_no
);
"#;
