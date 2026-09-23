//! Paragraph + audio-segment persistence ([`ParagraphStore`]) — the rows
//! written by the engine's live transcription. Child module of `sqlite` so it
//! shares the private connection handle.

use rusqlite::{params, OptionalExtension};

use crate::storage::{
    AudioSource, NewAudioSegment, Paragraph, ParagraphStore, StorageError,
};

use super::sql::{
    INSERT_AUDIO_SEGMENT, INSERT_PARAGRAPH, INSERT_TRANSCRIPT, RESET_PARAGRAPH,
    SELECT_LAST_TRANSCRIPT_SEQ, SELECT_PARAGRAPH, UPDATE_PARAGRAPH_APPEND,
};
use super::{parse_utc, to_rfc3339, SqliteStorage};

impl ParagraphStore for SqliteStorage {
    fn add_segment(&self, seg: &NewAudioSegment) -> Result<(), StorageError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let now = to_rfc3339(seg.created_at);

        // Fold into the session's single paragraph row, creating it on first
        // use; then insert the audio segment. Atomic so a segment is never
        // orphaned from the paragraph.
        if paragraph_exists(&tx, seg.session_id)? {
            tx.execute(
                UPDATE_PARAGRAPH_APPEND,
                params![seg.text, seg.end_ms, now, seg.session_id],
            )?;
        } else {
            tx.execute(
                INSERT_PARAGRAPH,
                params![
                    seg.session_id,
                    source_str(seg.source),
                    seg.text,
                    seg.start_ms,
                    seg.end_ms,
                    now,
                ],
            )?;
        }
        tx.execute(
            INSERT_AUDIO_SEGMENT,
            params![
                seg.session_id,
                source_str(seg.source),
                seg.text,
                seg.start_ms,
                seg.end_ms,
                seg.confidence,
                now,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn finalize_paragraph(&self, session_id: i64) -> Result<Option<(i64, String)>, StorageError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        let Some(para) = read_paragraph_opt(&tx, session_id)? else {
            return Ok(None);
        };
        if para.text.trim().is_empty() {
            return Ok(None);
        }

        // Move the paragraph text to the merged log (next sequence), then reset
        // the single row for the next paragraph.
        let seq: i64 = tx.query_row(SELECT_LAST_TRANSCRIPT_SEQ, params![session_id], |r| r.get(0))?;
        let seq = seq + 1;
        tx.execute(
            INSERT_TRANSCRIPT,
            params![
                session_id,
                source_str(para.source),
                para.text,
                para.start_ms,
                seq,
                to_rfc3339(chrono::Utc::now()),
            ],
        )?;
        tx.execute(
            RESET_PARAGRAPH,
            params![seq, to_rfc3339(chrono::Utc::now()), session_id],
        )?;
        tx.commit()?;
        Ok(Some((seq, para.text)))
    }
}

fn paragraph_exists(conn: &rusqlite::Connection, session_id: i64) -> Result<bool, StorageError> {
    let n: Option<i64> = conn
        .query_row("SELECT 1 FROM paragraphs WHERE session_id = ?1", params![session_id], |r| r.get(0))
        .optional()?;
    Ok(n.is_some())
}

fn read_paragraph_opt(
    conn: &rusqlite::Connection,
    session_id: i64,
) -> Result<Option<Paragraph>, StorageError> {
    let mut stmt = conn.prepare(SELECT_PARAGRAPH)?;
    let mut rows = stmt.query_map(params![session_id], read_paragraph)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

fn read_paragraph(row: &rusqlite::Row) -> rusqlite::Result<Paragraph> {
    let source: String = row.get(1)?;
    Ok(Paragraph {
        session_id: row.get(0)?,
        source: parse_source(&source),
        text: row.get(2)?,
        start_ms: row.get(3)?,
        end_ms: row.get(4)?,
        sequence_no: row.get(5)?,
        created_at: parse_utc(&row.get::<_, String>(6)?).map_err(row_err)?,
        updated_at: parse_utc(&row.get::<_, String>(7)?).map_err(row_err)?,
    })
}

fn row_err(e: StorageError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::other(e.to_string())),
    )
}

fn source_str(source: AudioSource) -> &'static str {
    match source {
        AudioSource::Mic => "mic",
        AudioSource::System => "system",
    }
}

fn parse_source(s: &str) -> AudioSource {
    match s {
        "mic" => AudioSource::Mic,
        _ => AudioSource::System,
    }
}
