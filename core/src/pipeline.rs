//! S7 pipeline: capture -> ring buffer -> background ASR thread -> typed events.
//!
//! ```text
//! source (capture thread) --push--> ring buffer --pop--> ASR thread --events--> consumer
//! ```
//!
//! Replaces the hand-rolled `roofle-cli stream` loop (S6) with a reusable,
//! threaded pipeline. A [`Source`] produces normalized 16 kHz mono audio on its
//! own capture thread (resample + gain live there), a bounded [`Ring`] carries
//! the samples to a dedicated ASR thread that drives a `sherpa-onnx` streaming
//! session and emits typed partial/final [`Segment`] events.
//!
//! The real-time audio callback never touches ASR inference — it only pushes raw
//! frames down a channel (see `audio::mic`); all heavy work (resample, inference,
//! VAD) happens on the pipeline's threads (plan: *never block the audio callback*).
//!
//! S9 wraps this in Tauri commands. S11/S12 implement [`Source`] for macOS and
//! Windows system audio behind `cfg(target_os = "...")`.

use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::asr::{self, Segment, StreamingSession};
use crate::audio::{mic, processing};

/// Default ring capacity in 16 kHz mono samples (~2 s). Absorbs scheduling
/// jitter between the capture and ASR threads without keeping much stale audio.
pub const DEFAULT_RING_CAPACITY: usize = 32_000;

/// ASR chunk size in 16 kHz samples (~0.1 s); partials update at this cadence.
const CHUNK_FRAMES: usize = 1_600;

/// How often an idle capture thread re-checks for shutdown.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Bounded circular buffer of 16 kHz mono `f32` samples.
///
/// The producer's [`push`](Ring::push) never blocks: when the buffer is full the
/// oldest samples are dropped, so a lagging ASR thread cannot stall realtime
/// capture. The consumer's [`pop_blocking`](Ring::pop_blocking) blocks while
/// empty and is woken by [`close`](Ring::close) at shutdown; after close + drain
/// it returns `None`.
#[derive(Clone)]
pub struct Ring {
    inner: Arc<Mutex<RingInner>>,
    wake: Arc<Condvar>,
}

struct RingInner {
    data: Vec<f32>,
    /// Index of the oldest buffered sample.
    start: usize,
    /// Number of buffered samples.
    len: usize,
    closed: bool,
}

impl Ring {
    /// Allocate a ring holding up to `capacity` samples.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "ring capacity must be > 0");
        Self {
            inner: Arc::new(Mutex::new(RingInner {
                data: vec![0.0; capacity],
                start: 0,
                len: 0,
                closed: false,
            })),
            wake: Arc::new(Condvar::new()),
        }
    }

    /// Whether [`close`](Self::close) has been called.
    pub fn is_closed(&self) -> bool {
        self.inner.lock().unwrap().closed
    }

    /// Append samples, dropping the oldest first when the ring is full.
    /// No-op once closed.
    pub fn push(&self, samples: &[f32]) {
        let mut inner = self.inner.lock().unwrap();
        if inner.closed {
            return;
        }
        let cap = inner.data.len();
        for &s in samples {
            let idx = (inner.start + inner.len) % cap;
            inner.data[idx] = s;
            if inner.len < cap {
                inner.len += 1;
            } else {
                inner.start = (inner.start + 1) % cap; // overwrite the oldest
            }
        }
        self.wake.notify_all();
    }

    /// Pop the oldest sample, blocking while the ring is empty. Returns `None`
    /// once the ring is closed and drained (the consumer should stop).
    pub fn pop_blocking(&self) -> Option<f32> {
        let mut inner = self.inner.lock().unwrap();
        while inner.len == 0 && !inner.closed {
            inner = self.wake.wait(inner).unwrap();
        }
        if inner.len == 0 {
            return None;
        }
        let s = inner.data[inner.start];
        inner.start = (inner.start + 1) % inner.data.len();
        inner.len -= 1;
        Some(s)
    }

    /// Mark the ring closed: pushes become no-ops and a blocked consumer wakes
    /// to drain what remains before returning `None`.
    pub fn close(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.closed = true;
        self.wake.notify_all();
    }
}

/// Errors from a [`Source`] (capture or normalization).
#[derive(Debug)]
pub enum SourceError {
    Capture(mic::CaptureError),
    Processing(processing::ProcessingError),
    /// Platform system-audio capture failed (e.g. Screen Recording denied).
    System(String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::Capture(e) => write!(f, "capture: {e}"),
            SourceError::Processing(e) => write!(f, "processing: {e}"),
            SourceError::System(e) => write!(f, "system audio: {e}"),
        }
    }
}

impl Error for SourceError {}

impl From<mic::CaptureError> for SourceError {
    fn from(e: mic::CaptureError) -> Self {
        SourceError::Capture(e)
    }
}

impl From<processing::ProcessingError> for SourceError {
    fn from(e: processing::ProcessingError) -> Self {
        SourceError::Processing(e)
    }
}

/// Errors from building or running a [`Pipeline`].
#[derive(Debug)]
pub enum PipelineError {
    Asr(asr::AsrError),
    Source(SourceError),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineError::Asr(e) => write!(f, "asr: {e}"),
            PipelineError::Source(e) => write!(f, "source: {e}"),
        }
    }
}

impl Error for PipelineError {}

impl From<asr::AsrError> for PipelineError {
    fn from(e: asr::AsrError) -> Self {
        PipelineError::Asr(e)
    }
}

impl From<SourceError> for PipelineError {
    fn from(e: SourceError) -> Self {
        PipelineError::Source(e)
    }
}

/// A producer of 16 kHz mono audio for the pipeline.
///
/// S7 ships the microphone implementation ([`MicSource`]); macOS system audio
/// (S11) and Windows WASAPI (S12) implement the same trait behind
/// `cfg(target_os = "...")`.
///
/// The source stays on the pipeline's thread for its whole lifetime — this is
/// deliberate: `cpal` streams are not `Send` on all platforms, so the capture
/// handle must not cross threads. [`start`](Source::start) runs on the caller's
/// thread (device errors surface synchronously from [`Pipeline::start`]) and
/// spawns the source's own capture thread that pushes samples into `ring`.
/// [`stop`](Source::stop) ends capture and joins that thread.
pub trait Source {
    /// Begin capture: open the device and start pushing 16 kHz mono samples
    /// into `ring`. Device errors surface here, on the caller's thread.
    fn start(&mut self, ring: Ring) -> Result<(), SourceError>;

    /// Stop capture and join the capture thread. Infallible; pump errors are
    /// logged on the capture thread.
    fn stop(&mut self);

    /// Pause or resume feeding audio into the ring. While paused the source
    /// keeps draining its capture channel (so the real-time audio callback never
    /// blocks) but discards the audio instead of buffering it. Default: no-op.
    fn set_paused(&mut self, _paused: bool) {}
}

/// macOS system-audio [`Source`] (S11); alias keeps call sites uniform.
#[cfg(target_os = "macos")]
pub use crate::audio::system_macos::SystemSource;

/// Microphone [`Source`]: cpal capture resampled to 16 kHz mono.
pub struct MicSource {
    /// Requested device name (`None` = system default).
    device: Option<String>,
    /// Resolved device name for display, from construction.
    resolved_name: String,
    /// Amplitude gain applied before the ring (1.0 = none).
    gain: f32,
    /// cpal capture handle. Non-`Send` on macOS, so it stays on this thread.
    capture: Option<mic::MicCapture>,
    thread: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    /// When true the pump discards captured audio instead of feeding the ring.
    paused: Arc<AtomicBool>,
}

impl MicSource {
    /// `device` of `None` selects the system-default input device.
    pub fn new(device: Option<String>, gain: f32) -> Self {
        let resolved_name = match &device {
            Some(n) => n.clone(),
            None => mic::default_input_name().unwrap_or_else(|| "<none>".to_string()),
        };
        Self {
            device,
            resolved_name,
            gain,
            capture: None,
            thread: None,
            stop: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Resolved input device name (for CLI/UI display).
    pub fn device_name(&self) -> &str {
        &self.resolved_name
    }
}

impl Source for MicSource {
    fn start(&mut self, ring: Ring) -> Result<(), SourceError> {
        let (tx, rx) = mpsc::channel::<mic::Frame>();
        let capture = match &self.device {
            Some(name) => mic::open_named(name, tx)?,
            None => mic::open_default(tx)?,
        };
        capture.play()?;
        let sample_rate = capture.sample_rate();
        let gain = self.gain;
        let stop = self.stop.clone();
        let paused = self.paused.clone();
        let ring = ring.clone();
        self.thread = Some(std::thread::spawn(move || {
            pump_loop(rx, sample_rate, gain, ring, stop, paused);
        }));
        self.capture = Some(capture);
        Ok(())
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Dropping the cpal stream stops capture and disconnects the pump's
        // receiver — a second signal for the pump thread to exit.
        self.capture = None;
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }

    fn set_paused(&mut self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
    }
}

/// Resample raw mic frames to 16 kHz mono and push them into the ring. Runs on
/// the source's pump thread — never the cpal audio callback.
fn pump_loop(
    frames: mpsc::Receiver<mic::Frame>,
    sample_rate: u32,
    gain: f32,
    ring: Ring,
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
) {
    let mut resampler = match processing::Resampler::new(sample_rate) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[roofle-core] pump resampler: {e}");
            return;
        }
    };
    while !stop.load(Ordering::SeqCst) && !ring.is_closed() {
        match frames.recv_timeout(POLL_INTERVAL) {
            Ok(frame) => {
                // While paused, keep draining the capture channel so the audio
                // callback never blocks, but drop the samples before they reach
                // the ring (and the ASR thread).
                if paused.load(Ordering::SeqCst) {
                    continue;
                }
                let mut mono = match resampler.process(&frame.samples) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("[roofle-core] pump resample: {e}");
                        break;
                    }
                };
                if gain != 1.0 {
                    processing::apply_gain(&mut mono, gain);
                }
                if !mono.is_empty() {
                    ring.push(&mono);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // Flush the resampler tail so the last ~10 ms is not lost.
    if let Ok(tail) = resampler.flush() {
        if !tail.is_empty() {
            ring.push(&tail);
        }
    }
}

/// Mic + system audio mixed into one stream.
///
/// Runs both the microphone and the system-audio capture simultaneously and
/// feeds both into the same ring buffer, so the pipeline transcribes them as a
/// single merged transcript (per design.md: one merged stream in v1). Echo from
/// system audio re-entering the mic is inherent to this mode; recommend
/// headphones. On platforms without system capture it surfaces an error on
/// [`start`](Source::start).
pub struct BothSource {
    mic: MicSource,
    #[cfg(target_os = "macos")]
    system: SystemSource,
}

impl BothSource {
    pub fn new(gain: f32) -> Self {
        Self {
            mic: MicSource::new(None, gain),
            #[cfg(target_os = "macos")]
            system: SystemSource::new(gain),
        }
    }
}

#[cfg(target_os = "macos")]
impl Source for BothSource {
    fn start(&mut self, ring: Ring) -> Result<(), SourceError> {
        // Start system audio first so a Screen Recording permission failure
        // surfaces before the mic is left running, then the microphone.
        self.system.start(ring.clone())?;
        self.mic.start(ring)?;
        Ok(())
    }

    fn stop(&mut self) {
        self.mic.stop();
        self.system.stop();
    }

    fn set_paused(&mut self, paused: bool) {
        self.mic.set_paused(paused);
        self.system.set_paused(paused);
    }
}

#[cfg(not(target_os = "macos"))]
impl Source for BothSource {
    fn start(&mut self, _ring: Ring) -> Result<(), SourceError> {
        Err(SourceError::System(
            "system audio is not implemented on this platform".to_string(),
        ))
    }

    fn stop(&mut self) {
        self.mic.stop();
    }

    fn set_paused(&mut self, paused: bool) {
        self.mic.set_paused(paused);
    }
}

/// Build-time options for a [`Pipeline`].
pub struct PipelineConfig {
    /// Directory containing the streaming ASR model files.
    pub model_dir: PathBuf,
    /// Audio source to transcribe.
    pub source: Box<dyn Source>,
    /// Ring capacity in 16 kHz mono samples; defaults to
    /// [`DEFAULT_RING_CAPACITY`] (~2 s).
    pub ring_capacity: Option<usize>,
}

/// A running transcription pipeline: a source's capture thread pushes 16 kHz
/// mono samples into a ring buffer, a dedicated ASR thread drains them through a
/// streaming recognizer and emits typed [`Segment`] events.
pub struct Pipeline {
    ring: Ring,
    engine: Option<asr::AsrEngine>,
    events_tx: Option<Sender<Segment>>,
    events_rx: Receiver<Segment>,
    levels_tx: Option<Sender<f32>>,
    levels_rx: Receiver<f32>,
    source: Option<Box<dyn Source>>,
    asr_thread: Option<JoinHandle<()>>,
    started: bool,
}

impl Pipeline {
    /// Load the ASR model and prepare the pipeline. No threads start until
    /// [`start`](Self::start).
    pub fn new(cfg: PipelineConfig) -> Result<Self, PipelineError> {
        let engine = asr::AsrEngine::new(asr::AsrModelConfig::from_model_dir(&cfg.model_dir))?;
        let (events_tx, events_rx) = mpsc::channel::<Segment>();
        let (levels_tx, levels_rx) = mpsc::channel::<f32>();
        let capacity = cfg.ring_capacity.unwrap_or(DEFAULT_RING_CAPACITY);
        Ok(Self {
            ring: Ring::new(capacity),
            engine: Some(engine),
            events_tx: Some(events_tx),
            events_rx,
            levels_tx: Some(levels_tx),
            levels_rx,
            source: Some(cfg.source),
            asr_thread: None,
            started: false,
        })
    }

    /// Start capture + the ASR thread. Idempotent. Device errors (e.g. no mic)
    /// surface here, on the caller's thread.
    pub fn start(&mut self) -> Result<(), PipelineError> {
        if self.started {
            return Ok(());
        }
        // Start the source first (it opens the device synchronously and begins
        // pushing into the ring), then run the ASR thread that drains it.
        let source = self.source.as_mut().expect("source present");
        source.start(self.ring.clone())?;

        let engine = self.engine.take().expect("engine already started");
        let ring = self.ring.clone();
        let tx = self
            .events_tx
            .as_ref()
            .expect("events sender present")
            .clone();
        let levels = self
            .levels_tx
            .as_ref()
            .expect("levels sender present")
            .clone();
        self.asr_thread = Some(std::thread::spawn(move || {
            asr_loop(engine, ring, tx, levels)
        }));

        self.started = true;
        Ok(())
    }

    /// Stream of typed transcription events (partial + final [`Segment`]s).
    pub fn events(&self) -> &Receiver<Segment> {
        &self.events_rx
    }

    /// Stream of per-chunk RMS levels (0.0..=~1.0), for a UI level meter.
    pub fn levels(&self) -> &Receiver<f32> {
        &self.levels_rx
    }

    /// Stop capture, flush trailing speech into a final segment, and join both
    /// threads. Idempotent.
    pub fn stop(&mut self) {
        self.shutdown();
    }

    /// Pause or resume the live source without tearing the pipeline down.
    /// While paused, captured audio is discarded before it reaches the ring, so
    /// the ASR session is preserved and can resume cleanly.
    pub fn set_paused(&mut self, paused: bool) {
        if let Some(source) = self.source.as_mut() {
            source.set_paused(paused);
        }
    }

    fn shutdown(&mut self) {
        if self.started {
            // Stop the source first so its resampler tail lands in the ring;
            // then close the ring so the ASR thread drains and finishes.
            if let Some(source) = self.source.as_mut() {
                source.stop();
            }
            self.ring.close();
            if let Some(handle) = self.asr_thread.take() {
                let _ = handle.join();
            }
            self.started = false;
        }
        // Drop the senders so a blocked consumer sees the stream end.
        self.events_tx = None;
        self.levels_tx = None;
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Drive the streaming recognizer from the ring buffer until it closes.
fn asr_loop(engine: asr::AsrEngine, ring: Ring, events: Sender<Segment>, levels: Sender<f32>) {
    let session = engine.stream();
    let mut chunk = Vec::with_capacity(CHUNK_FRAMES);
    let mut last_partial = String::new();

    while let Some(sample) = ring.pop_blocking() {
        chunk.push(sample);
        if chunk.len() == CHUNK_FRAMES {
            let _ = levels.send(rms(&chunk));
            process_chunk(&session, &chunk, &events, &mut last_partial);
            chunk.clear();
        }
    }
    if !chunk.is_empty() {
        let _ = levels.send(rms(&chunk));
        process_chunk(&session, &chunk, &events, &mut last_partial);
    }
    // End of input: finalize any trailing speech.
    let text = session.finish();
    if !text.is_empty() {
        let _ = events.send(Segment::final_(text));
    }
}

/// Root-mean-square amplitude of a sample block, 0.0..=~1.0.
fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

/// Feed one 16 kHz mono chunk to the session and emit any resulting events: a
/// changed partial, then a final per detected utterance boundary.
fn process_chunk(
    session: &StreamingSession,
    samples: &[f32],
    events: &Sender<Segment>,
    last_partial: &mut String,
) {
    session.feed(samples);

    let partial = session.partial();
    if !partial.is_empty() && partial != *last_partial {
        let _ = events.send(Segment::partial(&partial));
        *last_partial = partial;
    }

    while session.is_endpoint() {
        let text = session.take_final();
        // A final supersedes any previously emitted partial.
        last_partial.clear();
        if !text.is_empty() {
            let _ = events.send(Segment::final_(text));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::SegmentKind;
    use std::path::{Path, PathBuf};

    /// Dev model + test WAVs, downloaded by `scripts/fetch-assets.sh`. CI has no
    /// network models, so model-dependent tests skip (trivially pass) when
    /// absent.
    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn assets_present() -> bool {
        let root = repo_root();
        let model = root.join("assets/models/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17");
        let wav = root.join("assets/wavs/1.wav");
        model.join(asr::AsrModelConfig::ENCODER_FILE).exists()
            && model.join(asr::AsrModelConfig::DECODER_FILE).exists()
            && model.join(asr::AsrModelConfig::JOINER_FILE).exists()
            && model.join(asr::AsrModelConfig::TOKENS_FILE).exists()
            && wav.exists()
    }

    // --- Ring -----------------------------------------------------------

    #[test]
    fn ring_preserves_push_order() {
        let r = Ring::new(8);
        r.push(&[1.0, 2.0, 3.0]);
        assert_eq!(r.pop_blocking(), Some(1.0));
        assert_eq!(r.pop_blocking(), Some(2.0));
        assert_eq!(r.pop_blocking(), Some(3.0));
        r.close();
        assert_eq!(r.pop_blocking(), None);
    }

    #[test]
    fn ring_drops_oldest_when_full() {
        let r = Ring::new(3);
        r.push(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(r.pop_blocking(), Some(3.0));
        assert_eq!(r.pop_blocking(), Some(4.0));
        assert_eq!(r.pop_blocking(), Some(5.0));
        r.close();
        assert_eq!(r.pop_blocking(), None);
    }

    #[test]
    fn ring_push_after_close_is_noop() {
        let r = Ring::new(8);
        r.close();
        r.push(&[1.0]);
        assert_eq!(r.pop_blocking(), None);
    }

    #[test]
    fn ring_close_wakes_blocked_consumer() {
        let r = Ring::new(8);
        let h2 = r.clone();
        let handle = std::thread::spawn(move || h2.pop_blocking());
        std::thread::sleep(Duration::from_millis(20));
        r.close();
        assert_eq!(handle.join().unwrap(), None);
    }

    #[test]
    fn ring_push_wakes_blocked_consumer() {
        let r = Ring::new(8);
        let h2 = r.clone();
        let handle = std::thread::spawn(move || h2.pop_blocking());
        std::thread::sleep(Duration::from_millis(20));
        r.push(&[42.0]);
        assert_eq!(handle.join().unwrap(), Some(42.0));
        r.close();
    }

    // --- Pipeline -------------------------------------------------------

    /// A [`Source`] that pushes one pre-recorded 16 kHz mono buffer on start.
    /// Stands in for a microphone in tests.
    struct FakeSource {
        samples: Vec<f32>,
    }

    impl Source for FakeSource {
        fn start(&mut self, ring: Ring) -> Result<(), SourceError> {
            ring.push(&self.samples);
            Ok(())
        }

        fn stop(&mut self) {}
    }

    /// S7 gate: feeding the test WAV through the threaded pipeline (capture
    /// thread -> ring -> ASR thread) reproduces the same phrases as the S5/S6
    /// ASR tests.
    #[test]
    fn pipeline_transcribes_fed_audio() {
        if !assets_present() {
            eprintln!("SKIP: run `scripts/fetch-assets.sh` to download the S5 test model + WAV");
            return;
        }
        let root = repo_root();
        let model_dir =
            root.join("assets/models/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17");
        let wav = root.join("assets/wavs/1.wav");
        let wave = sherpa_onnx::Wave::read(wav.to_str().unwrap()).expect("read wav");

        let mut pipe = Pipeline::new(PipelineConfig {
            model_dir,
            source: Box::new(FakeSource {
                samples: wave.samples().to_vec(),
            }),
            // Room for the whole WAV: the fake source pushes it all on start.
            ring_capacity: Some(wave.samples().len() + 4096),
        })
        .expect("build pipeline");
        pipe.start().expect("start pipeline");
        pipe.stop();

        let text: String = pipe
            .events()
            .try_iter()
            .filter(|s| s.kind == SegmentKind::Final)
            .map(|s| s.text.clone())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();

        for phrase in [
            "direct consequence of the sin",
            "whose place was on that same",
            "connect her parent for ever",
            "blessed soul in heaven",
        ] {
            assert!(
                text.contains(phrase),
                "expected phrase {phrase:?} in pipeline transcript: {text:?}"
            );
        }
    }
}
