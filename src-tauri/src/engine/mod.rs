//! The engine thread. roofle-core's `Pipeline` is not `Send` (cpal handles on
//! macOS), so one thread owns it; the rest of the app talks to it over a
//! control channel and receives results as Tauri events.

mod clock;
mod control;
mod ingest;
mod pipeline;
mod session;
mod transcript_log;

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use roofle_core::pipeline::Pipeline;
use tauri::{AppHandle, Manager};

use crate::agents;
use crate::state::AppState;
use clock::RecordingClock;
use ingest::Ingest;

pub use control::{Control, Reply, SourceKind};

const POLL_INTERVAL: Duration = Duration::from_millis(20);

pub fn spawn_engine(app: AppHandle) -> Sender<Control> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || Engine::new(app).run(rx));
    tx
}

struct Engine {
    app: AppHandle,
    pipeline: Option<Pipeline>,
    clock: RecordingClock,
    ingest: Ingest,
    active_session: Option<i64>,
    /// Set on stop; the session is closed on the next loop pass.
    pending_finalize: Option<(i64, f64)>,
}

impl Engine {
    fn new(app: AppHandle) -> Self {
        Self {
            app,
            pipeline: None,
            clock: RecordingClock::default(),
            ingest: Ingest::default(),
            active_session: None,
            pending_finalize: None,
        }
    }

    fn run(mut self, rx: Receiver<Control>) {
        loop {
            match rx.recv_timeout(POLL_INTERVAL) {
                Ok(Control::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(control) => self.handle(control),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.drain_pipeline();
            self.finalize_if_stopped();
        }
        self.shutdown();
    }

    fn handle(&mut self, control: Control) {
        match control {
            Control::Start { source, session_id, reply } => {
                let _ = reply.send(self.start(source, session_id));
            }
            Control::Stop { reply } => {
                let _ = reply.send(self.stop());
            }
            Control::Pause { reply } => {
                let result = self.set_paused(true);
                if result.is_ok() {
                    self.clock.pause();
                }
                let _ = reply.send(result);
            }
            Control::Resume { reply } => {
                let result = self.set_paused(false);
                if result.is_ok() {
                    self.clock.resume();
                }
                let _ = reply.send(result);
            }
            Control::Shutdown => {}
        }
    }

    fn start(&mut self, source: SourceKind, session_id: Option<i64>) -> Result<(), String> {
        if self.pipeline.is_some() {
            return Err("already recording".to_string());
        }
        self.pipeline = Some(pipeline::start(&self.app, source)?);
        self.clock.start();
        self.app.state::<AppState>().transcript.lock().unwrap().clear();

        // Abort rather than record with no session row to stream into.
        match session::prepare(&self.app, source, session_id) {
            Ok(started) => {
                self.active_session = Some(started.id);
                let limit = self.app.state::<AppState>().char_limit.load(std::sync::atomic::Ordering::Relaxed);
                self.ingest.begin_session(started.timestamp_offset, limit);
                Ok(())
            }
            Err(e) => {
                if let Some(pipeline) = self.pipeline.take() {
                    pipeline::stop(pipeline);
                }
                self.clock.finish();
                Err(e)
            }
        }
    }

    fn stop(&mut self) -> Result<(), String> {
        let pipeline = self.pipeline.take().ok_or("not recording")?;
        pipeline::stop(pipeline);
        let duration = self.clock.finish();
        self.pending_finalize = self.active_session.map(|id| (id, duration));
        Ok(())
    }

    fn set_paused(&mut self, paused: bool) -> Result<(), String> {
        let pipeline = self.pipeline.as_mut().ok_or("not recording")?;
        pipeline.set_paused(paused);
        Ok(())
    }

    fn drain_pipeline(&mut self) {
        if let Some(pipeline) = &self.pipeline {
            self.ingest.drain(&self.app, pipeline, &self.clock, self.active_session);
        }
    }

    fn finalize_if_stopped(&mut self) {
        let Some((id, duration)) = self.pending_finalize.take() else {
            return;
        };
        self.ingest.flush(&self.app, self.active_session);
        session::close(&self.app, id, duration);
        agents::title::rename_session(self.app.clone(), id);
        self.active_session = None;
    }

    fn shutdown(mut self) {
        if let Some(pipeline) = self.pipeline.take() {
            pipeline::stop(pipeline);
        }
        let session_id = self.active_session.take();
        self.ingest.flush(&self.app, session_id);
        if let Some(id) = session_id {
            session::close(&self.app, id, self.clock.finish());
            agents::title::rename_session(self.app.clone(), id);
        }
    }
}
