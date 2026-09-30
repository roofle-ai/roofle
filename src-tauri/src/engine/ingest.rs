use chrono::Utc;
use live_agent::{ParagraphWatcher, DEFAULT_CHAR_LIMIT};
use roofle_core::asr::SegmentKind;
use roofle_core::pipeline::Pipeline;
use roofle_core::storage::{
    AudioSource, NewAudioSegment, NewSegment, ParagraphStore, SessionRepository,
};
use tauri::{AppHandle, Emitter, Manager};

use super::clock::RecordingClock;
use super::transcript_log::log_utterance;
use crate::agents;
use crate::events;
use crate::state::{AppState, TranscriptCue};

/// Turns pipeline output into UI events, stored segments and paragraphs; each
/// finished paragraph triggers the notes and challenge agents.
#[derive(Default)]
pub struct Ingest {
    seq: i64,
    timestamp_offset: f64,
    watcher: Option<ParagraphWatcher>,
}

impl Ingest {
    pub fn begin_session(&mut self, timestamp_offset: f64, last_seq: i64) {
        self.seq = last_seq;
        self.timestamp_offset = timestamp_offset;
        self.watcher = Some(ParagraphWatcher::new(DEFAULT_CHAR_LIMIT));
    }

    /// Relay pending segments and levels to the UI, persisting finalized ones.
    pub fn drain(
        &mut self,
        app: &AppHandle,
        pipeline: &Pipeline,
        clock: &RecordingClock,
        session_id: Option<i64>,
    ) {
        let mut pending: Vec<NewSegment> = Vec::new();
        for segment in pipeline.events().try_iter() {
            let _ = app.emit(events::TRANSCRIPT, segment.clone());
            if segment.kind != SegmentKind::Final {
                continue;
            }
            log_utterance(segment.source, &segment.text);
            let start_secs = self.timestamp_offset + clock.elapsed_secs();
            app.state::<AppState>().transcript.lock().unwrap().push(TranscriptCue {
                start_secs,
                source: segment.source,
                text: segment.text.clone(),
            });
            if let Some(sid) = session_id {
                self.seq += 1;
                pending.push(NewSegment {
                    session_id: sid,
                    seq: self.seq,
                    start_secs,
                    text: segment.text.clone(),
                    created_at: Utc::now(),
                });
                self.ingest_utterance(app, sid, segment.source, &segment.text, start_secs);
            }
        }
        if !pending.is_empty() {
            if let Err(e) = app.state::<AppState>().storage.append_segments(&pending) {
                eprintln!("[roofle] persist segments: {e}");
            }
        }
        for level in pipeline.levels().try_iter() {
            let _ = app.emit(events::LEVEL, level);
        }
    }

    /// Fold any buffered text into a last paragraph (even below the char
    /// limit) and drop the watcher.
    pub fn flush(&mut self, app: &AppHandle, session_id: Option<i64>) {
        let Some(watcher) = self.watcher.take() else {
            return;
        };
        if let (true, Some(sid)) = (watcher.has_pending(), session_id) {
            finalize_paragraph(app, sid);
        }
    }

    fn ingest_utterance(
        &mut self,
        app: &AppHandle,
        session_id: i64,
        source: AudioSource,
        text: &str,
        start_secs: f64,
    ) {
        let start_ms = (start_secs * 1000.0) as i64;
        let stored = app.state::<AppState>().storage.add_segment(&NewAudioSegment {
            session_id,
            source,
            text: text.to_string(),
            start_ms,
            // The ASR stream has no per-segment duration yet; keep end >= start.
            end_ms: start_ms,
            confidence: None,
            created_at: Utc::now(),
        });
        if let Err(e) = stored {
            return eprintln!("[roofle] persist audio segment: {e}");
        }

        if let Some(watcher) = self.watcher.as_mut() {
            if watcher.push(text) {
                finalize_paragraph(app, session_id);
                watcher.reset();
            }
        }
    }
}

fn finalize_paragraph(app: &AppHandle, session_id: i64) {
    let storage = &app.state::<AppState>().storage;
    match storage.finalize_paragraph(session_id) {
        Ok(Some((seq, text))) => {
            agents::notes::generate(app.clone(), session_id, seq, text.clone());
            agents::challenges::generate(app.clone(), session_id, seq, text);
        }
        Ok(None) => {}
        Err(e) => eprintln!("[roofle] finalize paragraph: {e}"),
    }
}
