//! Merged transcript-log persistence ([`TranscriptStore`]). Child module of
//! `sqlite` so it shares the private connection handle.

use rusqlite::params;

use crate::storage::{AudioSource, StorageError, TranscriptEntry, TranscriptStore};

use super::sql::{
    INSERT_TRANSCRIPT, SELECT_LAST_TRANSCRIPT_SEQ, SELECT_NEWEST_TRANSCRIPT,
    SELECT_TRANSCRIPT_AFTER,
};
use super::{parse_utc, to_rfc3339, SqliteStorage};

impl TranscriptStore for SqliteStorage {
    fn last_transcript_sequence(&self, session_id: i64) -> Result<i64, StorageError> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row(
            SELECT_LAST_TRANSCRIPT_SEQ,
            params![session_id],
            |r| r.get(0),
        )?)
    }

    fn append_transcript(
        &self,
        session_id: i64,
        source: AudioSource,
        text: &str,
        start_ms: i64,
    ) -> Result<i64, StorageError> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let seq: i64 = tx.query_row(SELECT_LAST_TRANSCRIPT_SEQ, params![session_id], |r| r.get(0))?;
        let seq = seq + 1;
        tx.execute(
            INSERT_TRANSCRIPT,
            params![
                session_id,
                source_str(source),
                text,
                start_ms,
                seq,
                to_rfc3339(chrono::Utc::now()),
            ],
        )?;
        tx.commit()?;
        Ok(seq)
    }

    fn newest_transcript(&self, session_id: i64) -> Result<Option<TranscriptEntry>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(SELECT_NEWEST_TRANSCRIPT)?;
        let mut rows = stmt.query_map(params![session_id], read_entry)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    fn transcript_after(
        &self,
        session_id: i64,
        after_sequence: i64,
    ) -> Result<Vec<TranscriptEntry>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(SELECT_TRANSCRIPT_AFTER)?;
        let rows = stmt.query_map(params![session_id, after_sequence], read_entry)?;
        rows.map(|r| r.map_err(StorageError::from)).collect()
    }
}

fn read_entry(row: &rusqlite::Row) -> rusqlite::Result<TranscriptEntry> {
    let source: String = row.get(2)?;
    Ok(TranscriptEntry {
        id: row.get(0)?,
        session_id: row.get(1)?,
        source: parse_source(&source),
        text: row.get(3)?,
        start_ms: row.get(4)?,
        sequence_no: row.get(5)?,
        created_at: parse_utc(&row.get::<_, String>(6)?)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(e.to_string()))))?,
    })
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
