use std::collections::HashMap;

use chrono::Utc;
use live_agent::{ParagraphWatcher, DEFAULT_CHAR_LIMIT};
use roofle_core::asr::SegmentKind;
use roofle_core::pipeline::Pipeline;
use roofle_core::storage::{AudioSource, NewAudioSegment, ParagraphStore};
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
    timestamp_offset: f64,
    watcher: Option<ParagraphWatcher>,
    /// Session-time (secs) at which each source's in-progress utterance began,
    /// recorded on the first partial and cleared on the final. Lets the stored
    /// `start_ms` reflect the utterance onset instead of the finalize time.
    utterance_start: HashMap<AudioSource, f64>,
}

impl Ingest {
    pub fn begin_session(&mut self, timestamp_offset: f64) {
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
        for segment in pipeline.events().try_iter() {
            let _ = app.emit(events::TRANSCRIPT, segment.clone());
            let now_secs = self.timestamp_offset + clock.elapsed_secs();
            match segment.kind {
                // A partial opens the utterance: keep the earliest hypothesis
                // time as its onset (later partials must not move it).
                SegmentKind::Partial => {
                    self.utterance_start.entry(segment.source).or_insert(now_secs);
                }
                SegmentKind::Final => {
                    log_utterance(segment.source, &segment.text);
                    // No preceding partial (offline Whisper emits finals only) =>
                    // the utterance is a point event: start = end = finalize time.
                    let start_secs = self
                        .utterance_start
                        .remove(&segment.source)
                        .unwrap_or(now_secs);
                    app.state::<AppState>().transcript.lock().unwrap().push(TranscriptCue {
                        start_secs,
                        source: segment.source,
                        text: segment.text.clone(),
                    });
                    if let Some(sid) = session_id {
                        self.ingest_utterance(
                            app,
                            sid,
                            segment.source,
                            &segment.text,
                            start_secs,
                            now_secs,
                        );
                    }
                }
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
        end_secs: f64,
    ) {
        let start_ms = (start_secs * 1000.0) as i64;
        // A partial may land ~the same instant as the final; keep end >= start
        // so the schema CHECK holds.
        let end_ms = ((end_secs * 1000.0) as i64).max(start_ms);
        let stored = app.state::<AppState>().storage.add_segment(&NewAudioSegment {
            session_id,
            source,
            text: text.to_string(),
            start_ms,
            end_ms,
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
