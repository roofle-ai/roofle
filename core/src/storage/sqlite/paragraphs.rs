//! Paragraph persistence ([`ParagraphStore`]) — the rows written by
//! live-agent's paragraph watcher. Child module of `sqlite` so it shares the
//! private connection handle.

use rusqlite::params;

use crate::storage::{NewParagraph, Paragraph, ParagraphStore, StorageError};

use super::sql::{INSERT_PARAGRAPH, SELECT_PARAGRAPHS};
use super::{parse_utc, to_rfc3339, SqliteStorage};

impl ParagraphStore for SqliteStorage {
    fn create_paragraph(&self, input: NewParagraph) -> Result<Paragraph, StorageError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            INSERT_PARAGRAPH,
            params![
                input.session_id,
                input.seq,
                input.prev_text,
                input.new_text,
                to_rfc3339(input.created_at),
            ],
        )?;
        Ok(Paragraph {
            id: conn.last_insert_rowid(),
            session_id: input.session_id,
            seq: input.seq,
            prev_text: input.prev_text,
            new_text: input.new_text,
            created_at: input.created_at,
        })
    }

    fn list_paragraphs(&self, session_id: i64) -> Result<Vec<Paragraph>, StorageError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(SELECT_PARAGRAPHS)?;
        let rows = stmt.query_map(params![session_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?;
        rows.map(|r| {
            let (id, sid, seq, prev, new, created) = r?;
            Ok(Paragraph {
                id,
                session_id: sid,
                seq,
                prev_text: prev,
                new_text: new,
                created_at: parse_utc(&created)?,
            })
        })
        .collect()
    }
}
