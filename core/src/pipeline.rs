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
use crate::storage::AudioSource;

/// Default ring capacity in 16 kHz mono samples (~2 s). Absorbs scheduling
/// jitter between the capture and ASR threads without keeping much stale audio.
pub const DEFAULT_RING_CAPACITY: usize = 32_000;

/// Minimum ring capacity (~90 s) used when the engine is offline Whisper: the
/// ASR thread stops popping while it decodes a ~28 s window, so the ring must
/// absorb that whole decode-time inflow or speech would be dropped.
const WHISPER_RING_CAPACITY: usize = 90 * crate::asr::ASR_SAMPLE_RATE as usize;

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

    /// Pop the oldest sample, waiting up to `max` while the ring is empty.
    /// Returns `None` on timeout, or when the ring is closed and drained.
    /// Enables a single ASR thread to multiplex several labelled rings without
    /// blocking indefinitely on any one of them.
    pub fn pop_timeout(&self, max: Duration) -> Option<f32> {
        let mut inner = self.inner.lock().unwrap();
        let deadline = std::time::Instant::now() + max;
        while inner.len == 0 && !inner.closed {
            let now = std::time::Instant::now();
            if now >= deadline {
                return None;
            }
            let (guard, timeout) = self.wake.wait_timeout(inner, deadline - now).unwrap();
            inner = guard;
            if timeout.timed_out() {
                return None;
            }
        }
        if inner.len == 0 {
            return None;
        }
        let s = inner.data[inner.start];
        inner.start = (inner.start + 1) % inner.data.len();
        inner.len -= 1;
        Some(s)
    }

    /// Whether the ring currently holds no buffered samples.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().len == 0
    }

    /// Mark the ring closed: pushes become no-ops and a blocked consumer wakes
    /// to drain what remains before returning `None`.
    pub fn close(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.closed = true;
        self.wake.notify_all();
    }
}

/// A [`Ring`] bound to the capture source that feeds it.
///
/// The ASR layer keeps one streaming session per labelled ring and tags every
/// emitted [`Segment`] with its ring's [`AudioSource`], so mic and system audio
/// never lose their identity after being captured (the S7 design merged both
/// into one ring, destroying that information).
#[derive(Clone)]
pub struct LabelledRing {
    pub source: AudioSource,
    pub ring: Ring,
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
    /// The labelled capture streams this source produces, in the order
    /// [`start`](Self::start) receives their rings. Defaults to a single
    /// microphone stream. A source may return several (e.g. [`BothSource`]
    /// captures mic + system), in which case each ring is transcribed and
    /// labelled independently.
    fn stream_labels(&self) -> Vec<AudioSource> {
        vec![AudioSource::Mic]
    }

    /// Begin capture: open the device and start pushing 16 kHz mono samples
    /// into the labelled ring(s) whose [`AudioSource`] matches this source's
    /// [`stream_labels`](Self::stream_labels). Device errors surface here, on
    /// the caller's thread.
    fn start(&mut self, rings: &[LabelledRing]) -> Result<(), SourceError>;

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
    fn stream_labels(&self) -> Vec<AudioSource> {
        vec![AudioSource::Mic]
    }

    fn start(&mut self, rings: &[LabelledRing]) -> Result<(), SourceError> {
        let ring = labelled_ring(rings, AudioSource::Mic)
            .expect("mic source: labelled ring present")
            .ring
            .clone();
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

/// Find the [`LabelledRing`] a source should feed by its [`AudioSource`].
///
/// The pipeline constructs the rings from [`Source::stream_labels`], so a
/// matching ring is guaranteed to exist for every source.
fn labelled_ring<'a>(rings: &'a [LabelledRing], source: AudioSource) -> Option<&'a LabelledRing> {
    rings.iter().find(|r| r.source == source)
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

/// Mic + system audio captured into two separate, source-labelled streams.
///
/// Runs both the microphone and the system-audio capture simultaneously, each
/// feeding its own labelled [`Ring`]. The pipeline transcribes them as two
/// independent ASR streams and tags every emitted [`Segment`] with its source,
/// so the transcript shows which participant (mic vs system/speaker) said what.
/// On platforms without system capture it surfaces an error on
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
    fn stream_labels(&self) -> Vec<AudioSource> {
        vec![AudioSource::Mic, AudioSource::System]
    }

    fn start(&mut self, rings: &[LabelledRing]) -> Result<(), SourceError> {
        // Start system audio first so a Screen Recording permission failure
        // surfaces before the mic is left running, then the microphone. Each
        // source picks its own labelled ring, so mic and system stay separate.
        self.system.start(rings)?;
        self.mic.start(rings)?;
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
    fn stream_labels(&self) -> Vec<AudioSource> {
        vec![AudioSource::Mic, AudioSource::System]
    }

    fn start(&mut self, _rings: &[LabelledRing]) -> Result<(), SourceError> {
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
    /// Path to the Silero VAD ONNX model (`silero_vad.onnx`). Used only by the
    /// offline Whisper engine to split the stream into clean speech utterances
    /// before decoding (prevents Whisper's repetition/hallucination on raw
    /// windows). `None` falls back to fixed-window decoding.
    pub vad_model: Option<PathBuf>,
}

/// A running transcription pipeline: a source's capture thread pushes 16 kHz
/// mono samples into a ring buffer, a dedicated ASR thread drains them through a
/// streaming recognizer and emits typed [`Segment`] events.
pub struct Pipeline {
    /// One source-labelled ring per capture stream; the ASR thread keeps a
    /// separate session for each so segments carry their source.
    rings: Vec<LabelledRing>,
    engine: Option<asr::Engine>,
    /// Silero VAD model path for the offline Whisper engine. Kept as a path so
    /// one VAD per labelled ring can be constructed on the ASR thread (VAD is
    /// not `Clone`). Unused by the streaming engine.
    vad_model: Option<PathBuf>,
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
        let engine = asr::Engine::from_model_dir(&cfg.model_dir)?;
        let (events_tx, events_rx) = mpsc::channel::<Segment>();
        let (levels_tx, levels_rx) = mpsc::channel::<f32>();
        let mut capacity = cfg.ring_capacity.unwrap_or(DEFAULT_RING_CAPACITY);
        // Offline Whisper pauses the ASR thread while decoding each window, so
        // it needs a ring large enough to absorb the decode-time inflow.
        if engine.is_whisper() {
            capacity = capacity.max(WHISPER_RING_CAPACITY);
        }
        // Build one labelled ring per capture stream the source declares. The
        // ASR thread drains each ring into its own session so segments keep
        // their source identity.
        let labels = cfg.source.stream_labels();
        let rings = labels
            .into_iter()
            .map(|source| LabelledRing {
                source,
                ring: Ring::new(capacity),
            })
            .collect::<Vec<_>>();
        Ok(Self {
            rings,
            engine: Some(engine),
            // Keep the VAD model path; per-ring VADs are built on the ASR thread
            // because the streaming engine doesn't use one and `Vad` isn't Clone.
            vad_model: cfg.vad_model,
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
        // pushing into the rings), then run the ASR thread that drains them.
        let source = self.source.as_mut().expect("source present");
        source.start(&self.rings)?;

        let engine = self.engine.take().expect("engine already started");
        let vad_model = self.vad_model.take();
        // Clone the labelled rings into the ASR thread and keep our own clones:
        // `Ring` is Arc-backed, so closing our copies closes the shared rings
        // the thread drains (and `shutdown` must close them on this struct).
        let rings = self.rings.clone();
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
        self.asr_thread = Some(std::thread::spawn(move || match engine {
            asr::Engine::Streaming(e) => streaming_asr_loop(e, rings, tx, levels),
            asr::Engine::Whisper(e) => whisper_asr_loop(e, vad_model, rings, tx, levels),
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
            // Stop the source first so its resampler tails land in the rings;
            // then close the rings so the ASR thread drains and finishes.
            if let Some(source) = self.source.as_mut() {
                source.stop();
            }
            for labelled in &self.rings {
                labelled.ring.close();
            }
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

/// How long the ASR thread waits on an empty ring before checking the others.
const MULTIPLEX_POLL: Duration = Duration::from_millis(2);

/// System RMS above which the speaker is treated as actively producing speech.
/// While it is (plus a hangover), the mic is presumed to be carrying echo, so
/// its audio is not transcribed at all (turn-taking capture).
const SYS_VAD_THRESHOLD: f32 = 0.01;
/// Echo-decay hangover: keep suppressing the mic for this many chunk-steps
/// after system speech ends, covering the acoustic echo tail in the mic.
const ECHO_HANGOVER: usize = 4; // ~400 ms at 0.1 s chunks

/// Turn-taking echo suppressor for the mic stream.
///
/// The microphone physically captures the user's voice AND an attenuated echo of
/// whatever the speakers play. No level comparison can reliably separate them
/// (echo can be anywhere from quiet to nearly as loud as the system signal), so
/// this suppressor implements turn-taking instead: while the system reference
/// indicates the speaker is active, the mic stream is dropped entirely. Since a
/// meeting is one person talking at a time, this keeps system audio out of the
/// "mic" transcript without needing full echo cancellation. Suppression lingers
/// for a short hangover after the system stops, covering the echo tail.
struct EchoSuppressor {
    /// Remaining chunk-steps of suppression while the system recently spoke.
    hangover: usize,
}

impl EchoSuppressor {
    fn new() -> Self {
        Self { hangover: 0 }
    }

    /// Feed the RMS of each system (speaker) chunk. Chunks above the VAD
    /// threshold arm the hangover; quiet chunks drain it one step at a time.
    fn observe_system(&mut self, rms: f32) {
        if rms > SYS_VAD_THRESHOLD {
            self.hangover = ECHO_HANGOVER;
        } else if self.hangover > 0 {
            self.hangover -= 1;
        }
    }

    /// Whether the mic stream should be fed to ASR: only when the system has not
    /// been speaking recently. The mic's own level is irrelevant here — echo is
    /// suppressed regardless of how loud it is.
    fn passes(&self, _mic_rms: f32) -> bool {
        self.hangover == 0
    }
}

/// Drive the streaming recognizer across one or more source-labelled rings until
/// they close. A single `OnlineRecognizer` drives one `OnlineStream` per ring;
/// each stream's events are tagged with its ring's [`AudioSource`] and merged
/// chronologically onto the shared event channel. A dominant-source echo gate
/// drops mic windows that are dominated by system echo.
fn streaming_asr_loop(
    engine: asr::AsrEngine,
    rings: Vec<LabelledRing>,
    events: Sender<Segment>,
    levels: Sender<f32>,
) {
    let n = rings.len();
    let mut sessions: Vec<(AudioSource, StreamingSession)> = Vec::with_capacity(n);
    let mut chunks: Vec<Vec<f32>> = Vec::with_capacity(n);
    let mut last_partials: Vec<String> = Vec::with_capacity(n);
    for labelled in &rings {
        sessions.push((labelled.source, engine.stream()));
        chunks.push(Vec::with_capacity(CHUNK_FRAMES));
        last_partials.push(String::new());
    }
    // Echo suppressor driven by the system (speaker) ring, used to gate the mic
    // stream. Both rings advance in real time, so the system reference is a good
    // proxy for what is concurrently playing on the speakers.
    let mut suppressor = EchoSuppressor::new();

    loop {
        let mut any_open = false;
        for i in 0..n {
            let ring = &rings[i].ring;
            if ring.is_closed() && ring.is_empty() {
                continue; // this stream fully drained
            }
            any_open = true;
            while let Some(sample) = ring.pop_timeout(MULTIPLEX_POLL) {
                chunks[i].push(sample);
                if chunks[i].len() == CHUNK_FRAMES {
                    let level = rms(&chunks[i]);
                    let _ = levels.send(level);
                    let source = sessions[i].0;
                    if source == AudioSource::System {
                        suppressor.observe_system(level);
                    } else if !suppressor.passes(level) {
                        chunks[i].clear();
                        continue;
                    }
                    process_chunk(
                        &sessions[i].1,
                        &chunks[i],
                        &events,
                        &mut last_partials[i],
                        source,
                    );
                    chunks[i].clear();
                }
            }
        }
        if !any_open {
            break;
        }
    }

    // Flush any trailing partial chunks per ring.
    for i in 0..n {
        if !chunks[i].is_empty() {
            let level = rms(&chunks[i]);
            let _ = levels.send(level);
            let source = sessions[i].0;
            if source == AudioSource::Mic && !suppressor.passes(level) {
                chunks[i].clear();
                continue;
            }
            process_chunk(&sessions[i].1, &chunks[i], &events, &mut last_partials[i], source);
            chunks[i].clear();
        }
    }
    // End of input: finalize any trailing speech per stream.
    for (source, session) in sessions.iter() {
        let text = session.finish();
        if !text.is_empty() {
            let _ = events.send(Segment::final_(*source, text));
        }
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

/// Target RMS for normalized audio fed to Whisper.
const TARGET_RMS: f32 = 0.08;

/// Scale a block to [`TARGET_RMS`] so quiet microphone audio reaches Whisper at
/// a consistent level (real mics are often much quieter than clean test WAVs,
/// and low level is a common cause of empty/hallucinated Whisper output). A
/// no-op on silence; gain is clamped so loud audio is not boosted into clipping.
fn normalize(samples: &[f32]) -> Vec<f32> {
    let r = rms(samples);
    if r < 1e-4 {
        return samples.to_vec();
    }
    let gain = (TARGET_RMS / r).min(4.0);
    samples.iter().map(|&s| (s * gain).clamp(-1.0, 1.0)).collect()
}

/// Feed one 16 kHz mono chunk to the session and emit any resulting events: a
/// changed partial, then a final per detected utterance boundary.
fn process_chunk(
    session: &StreamingSession,
    samples: &[f32],
    events: &Sender<Segment>,
    last_partial: &mut String,
    source: AudioSource,
) {
    session.feed(samples);

    let partial = session.partial();
    if !partial.is_empty() && partial != *last_partial {
        let _ = events.send(Segment::partial(source, &partial));
        *last_partial = partial;
    }

    while session.is_endpoint() {
        let text = session.take_final();
        // A final supersedes any previously emitted partial.
        last_partial.clear();
        if !text.is_empty() {
            let _ = events.send(Segment::final_(source, text));
        }
    }
}

/// Drive an offline Whisper engine across one or more source-labelled rings until
/// they close.
///
/// Whisper has no streaming hypotheses. With a [`Vad`] the incoming audio is
/// split into clean speech utterances and each is decoded offline into one
/// final [`Segment`] — dropping silence and avoiding the repetition /
/// hallucination Whisper shows when fed raw fixed-length windows. Without a VAD
/// it falls back to ~28 s windows (the Whisper context). One VAD (if configured)
/// and one window accumulator exist per ring; every emitted final is tagged with
/// its ring's [`AudioSource`]. Partial events are never emitted.
fn whisper_asr_loop(
    engine: asr::whisper::WhisperEngine,
    vad_model: Option<PathBuf>,
    rings: Vec<LabelledRing>,
    events: Sender<Segment>,
    levels: Sender<f32>,
) {
    let rate = engine.sample_rate();
    let block_cap = 1_600usize;

    let n = rings.len();
    let vads: Vec<Option<asr::vad::Vad>> = (0..n)
        .map(|_| {
            vad_model
                .as_ref()
                .map(|p| asr::vad::Vad::new(p).expect("load VAD for whisper ring"))
        })
        .collect();
    let mut blocks: Vec<Vec<f32>> = (0..n).map(|_| Vec::with_capacity(block_cap)).collect();
    let mut windows: Vec<Vec<f32>> = (0..n)
        .map(|_| Vec::with_capacity(asr::whisper::WINDOW_SAMPLES))
        .collect();
    let mut suppressor = EchoSuppressor::new();

    loop {
        let mut any_open = false;
        for i in 0..n {
            let ring = &rings[i].ring;
            if ring.is_closed() && ring.is_empty() {
                continue;
            }
            any_open = true;
            while let Some(sample) = ring.pop_timeout(MULTIPLEX_POLL) {
                blocks[i].push(sample);
                if blocks[i].len() >= block_cap {
                    let level = rms(&blocks[i]);
                    let _ = levels.send(level);
                    let source = rings[i].source;
                    if source == AudioSource::System {
                        suppressor.observe_system(level);
                    } else if !suppressor.passes(level) {
                        blocks[i].clear();
                        continue;
                    }
                    feed_whisper_block(&engine, &vads[i], &mut windows[i], source, &blocks[i], &events);
                    blocks[i].clear();
                }
            }
        }
        if !any_open {
            break;
        }
    }

    // End of input: flush trailing speech (VAD) or the partial window (fallback).
    for i in 0..n {
        let source = rings[i].source;
        // Feed any remaining partial block (echo-gated on the mic stream).
        if !blocks[i].is_empty() {
            let level = rms(&blocks[i]);
            if !(source == AudioSource::Mic && !suppressor.passes(level)) {
                feed_whisper_block(&engine, &vads[i], &mut windows[i], source, &blocks[i], &events);
            }
            blocks[i].clear();
        }
        match &vads[i] {
            Some(vad) => {
                for seg in vad.flush() {
                    if let Ok(text) = engine.transcribe(rate, &normalize(&seg)) {
                        let _ = events.send(Segment::final_(source, text));
                    }
                }
            }
            None => {
                if !windows[i].is_empty() {
                    let _ = levels.send(rms(&windows[i]));
                    if let Ok(text) = engine.transcribe(rate, &normalize(&windows[i])) {
                        let _ = events.send(Segment::final_(source, text));
                    }
                }
            }
        }
    }
}

/// Queue one ~0.1 s block into a ring's VAD or fixed-window accumulator and emit
/// any finalized utterances, tagged with the ring's `source`.
fn feed_whisper_block(
    engine: &asr::whisper::WhisperEngine,
    vad: &Option<asr::vad::Vad>,
    window: &mut Vec<f32>,
    source: AudioSource,
    block: &[f32],
    events: &Sender<Segment>,
) {
    let rate = engine.sample_rate();
    match vad {
        // VAD routing: queue the block, then decode each completed speech
        // utterance standalone.
        Some(vad) => {
            vad.push(block);
            for seg in vad.drain() {
                if let Ok(text) = engine.transcribe(rate, &normalize(&seg)) {
                    let _ = events.send(Segment::final_(source, text));
                }
            }
        }
        // Fallback routing: accumulate into fixed windows.
        None => {
            window.extend_from_slice(block);
            if window.len() >= asr::whisper::WINDOW_SAMPLES {
                if let Ok(text) = engine.transcribe(rate, &normalize(window)) {
                    let _ = events.send(Segment::final_(source, text));
                }
                window.clear();
            }
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
        let model = root.join("assets/models/sherpa-onnx-whisper-base.en");
        let wav = root.join("assets/wavs/1.wav");
        asr::model_present(&model) && wav.exists()
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

    #[test]
    fn ring_pop_timeout_returns_none_when_empty() {
        let r = Ring::new(8);
        // Empty and open: the poll times out without returning data.
        assert_eq!(r.pop_timeout(Duration::from_millis(10)), None);
        r.close();
        assert_eq!(r.pop_timeout(Duration::from_millis(10)), None);
    }

    #[test]
    fn ring_pop_timeout_returns_pushed_samples() {
        let r = Ring::new(8);
        r.push(&[7.0, 8.0]);
        assert_eq!(r.pop_timeout(Duration::from_millis(5)), Some(7.0));
        assert_eq!(r.pop_timeout(Duration::from_millis(5)), Some(8.0));
        assert_eq!(r.pop_timeout(Duration::from_millis(5)), None);
        r.close();
    }

    /// Regression: the pipeline keeps its own ring clones to close on shutdown
    /// while the ASR thread drains its own clones. Closing one clone must be
    /// observed by the other, or `shutdown` would block forever on `join`
    /// (which froze Stop/Pause). `Ring` is Arc-backed, so this holds by design.
    #[test]
    fn ring_close_is_shared_across_clones() {
        let pipeline_copy = Ring::new(8);
        let asr_thread_copy = pipeline_copy.clone(); // handed to the ASR thread
        pipeline_copy.push(&[1.0, 2.0]);
        // The pipeline closes its copy on shutdown; the ASR thread's copy must
        // then drain what remains and report closed/empty.
        pipeline_copy.close();
        assert_eq!(asr_thread_copy.pop_blocking(), Some(1.0));
        assert_eq!(asr_thread_copy.pop_blocking(), Some(2.0));
        assert_eq!(asr_thread_copy.pop_blocking(), None);
        assert!(asr_thread_copy.is_closed());
    }

    // --- Echo suppressor --------------------------------------------------

    /// With the system silent, the mic always passes (no echo to suppress).
    #[test]
    fn echo_suppressor_passes_mic_when_system_is_silent() {
        let mut s = EchoSuppressor::new();
        s.observe_system(0.002); // quiet, below the system-VAD threshold
        assert!(s.passes(0.001));
        assert!(s.passes(0.01));
        assert!(s.passes(0.05));
    }

    /// While the system is actively speaking the mic is dropped entirely,
    /// regardless of its level — echo is suppressed at any volume.
    #[test]
    fn echo_suppressor_drops_mic_while_system_speaks() {
        let mut s = EchoSuppressor::new();
        s.observe_system(0.1); // speaker actively talking
        assert!(!s.passes(0.02)); // quiet echo => dropped
        assert!(!s.passes(0.3)); // even loud mic (echo or overlap) => dropped
    }

    /// Quiet-but-active system speech must still suppress the mic (the old
    /// level gate let this through).
    #[test]
    fn echo_suppressor_handles_quiet_system_speech() {
        let mut s = EchoSuppressor::new();
        s.observe_system(0.02); // quiet but active system speech
        assert!(!s.passes(0.02)); // comparable mic => dropped
        assert!(!s.passes(0.06)); // louder mic => still dropped
    }

    /// Suppression lingers after system speech ends (echo tail), then releases.
    #[test]
    fn echo_suppressor_hangover_covers_echo_tail() {
        let mut s = EchoSuppressor::new();
        s.observe_system(0.1); // speaker talking
        s.observe_system(0.001); // speaker stops; hangover armed
        assert!(!s.passes(0.02));
        // Once the hangover drains, the user's turn may start again.
        for _ in 0..=ECHO_HANGOVER {
            s.observe_system(0.0);
        }
        assert!(s.passes(0.02));
    }

    // --- Segment source tagging ------------------------------------------

    #[test]
    fn segments_carry_their_capture_source() {
        let mic = Segment::final_(AudioSource::Mic, "hello");
        assert_eq!(mic.source, AudioSource::Mic);
        assert_eq!(mic.kind, SegmentKind::Final);

        let sys = Segment::partial(AudioSource::System, "par");
        assert_eq!(sys.source, AudioSource::System);
        assert_eq!(sys.kind, SegmentKind::Partial);
    }

    // --- Pipeline -------------------------------------------------------

    /// A [`Source`] that pushes one pre-recorded 16 kHz mono buffer on start.
    /// Stands in for a microphone in tests.
    struct FakeSource {
        samples: Vec<f32>,
    }

    impl Source for FakeSource {
        fn start(&mut self, rings: &[LabelledRing]) -> Result<(), SourceError> {
            let ring = labelled_ring(rings, AudioSource::Mic)
                .expect("fake source: labelled ring present")
                .ring
                .clone();
            ring.push(&self.samples);
            Ok(())
        }

        fn stop(&mut self) {}
    }

    /// S7 gate: feeding the test WAV through the threaded pipeline (capture
    /// thread -> ring -> ASR thread) reproduces the same phrases as the S5/S6
    /// ASR tests. Uses the Whisper engine (auto-selected from the model dir).
    #[test]
    fn pipeline_transcribes_fed_audio() {
        if !assets_present() {
            eprintln!("SKIP: run `scripts/fetch-assets.sh` to download the Whisper model + WAV");
            return;
        }
        let root = repo_root();
        let model_dir = root.join("assets/models/sherpa-onnx-whisper-base.en");
        let wav = root.join("assets/wavs/1.wav");
        let wave = sherpa_onnx::Wave::read(wav.to_str().unwrap()).expect("read wav");

        let mut pipe = Pipeline::new(PipelineConfig {
            model_dir,
            source: Box::new(FakeSource {
                samples: wave.samples().to_vec(),
            }),
            // The whisper engine bumps the ring to WHISPER_RING_CAPACITY, so
            // whatever we request here is overridden upward for the offline path.
            ring_capacity: Some(wave.samples().len() + 4096),
            // No VAD model in this offline test: falls back to fixed-window
            // decoding, which is what this test exercises.
            vad_model: None,
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

        for phrase in ["direct consequence", "blessed soul in heaven"] {
            assert!(
                text.contains(phrase),
                "expected phrase {phrase:?} in pipeline transcript: {text:?}"
            );
        }
    }

    /// Routing gate: with the Silero VAD enabled, the same WAV is segmented into
    /// clean speech utterances and decoded by Whisper (the default production
    /// routing that replaces fixed-window feeding).
    #[test]
    fn pipeline_transcribes_fed_audio_with_vad() {
        let root = repo_root();
        let vad = root.join("assets/models/silero_vad.onnx");
        if !assets_present() || !vad.is_file() {
            eprintln!("SKIP: whisper model + silero_vad.onnx required");
            return;
        }
        let model_dir = root.join("assets/models/sherpa-onnx-whisper-base.en");
        let wav = root.join("assets/wavs/1.wav");
        let wave = sherpa_onnx::Wave::read(wav.to_str().unwrap()).expect("read wav");

        let mut pipe = Pipeline::new(PipelineConfig {
            model_dir,
            source: Box::new(FakeSource {
                samples: wave.samples().to_vec(),
            }),
            ring_capacity: Some(wave.samples().len() + 4096),
            vad_model: Some(vad),
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

        assert!(
            text.contains("direct consequence"),
            "expected phrase in VAD-routed pipeline transcript: {text:?}"
        );
    }
}
