//! roofle-cli — headless spike binary.
//!
//! Exercises the `core/` pipeline in the terminal before any Tauri/UI work.
//!
//! - default: open the system-default input device and print a live level meter
//! - `--list`: enumerate input devices and exit
//! - `--device <name>`: open a specific input device
//! - `transcribe --wav <file>`: offline ASR over a WAV file (S5)
//! - `stream`: live mic -> pipeline (S6/S7), printing partial and final
//!   segments as typed events arrive from the background ASR thread

use std::error::Error;
use std::io::{self, Write};
use std::sync::mpsc;
use std::time::Duration;

use roofle_core::asr::{self, SegmentKind};
use roofle_core::audio::mic;
use roofle_core::pipeline;

/// Default streaming Zipformer-Transducer model (English, int8), downloaded by
/// `scripts/fetch-assets.sh`. See `design.md` Rev 2.2 for why it is the default.
const DEFAULT_MODEL_DIR: &str = "assets/models/sherpa-onnx-whisper-base.en";

/// Default amplitude gain applied to captured audio (no-op at 1.0).
const DEFAULT_GAIN: f32 = 1.0;

/// Render an RMS value (0.0..=1.0) as an ASCII meter of fixed width.
fn meter(rms: f32) -> String {
    const WIDTH: usize = 40;
    let filled = ((rms * WIDTH as f32) as usize).min(WIDTH);
    let mut s = String::with_capacity(WIDTH);
    for i in 0..WIDTH {
        s.push(if i < filled { '█' } else { '░' });
    }
    s
}

fn print_devices() -> Result<(), mic::CaptureError> {
    println!("Available input devices:");
    let names = mic::list_input_devices()?;
    let default = mic::default_input_name();
    for name in &names {
        let tag = if Some(name) == default.as_ref() {
            "  (default)"
        } else {
            ""
        };
        println!("  - {name}{tag}");
    }
    Ok(())
}

/// Read the value that follows `--flag` in `args`, if present.
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

/// Print a labeled segment. Partials overwrite the current line in place (a
/// growing hypothesis does not scroll); finals close the line and start a new
/// one below.
fn print_segment(out: &mut dyn Write, kind: SegmentKind, text: &str) -> io::Result<()> {
    match kind {
        SegmentKind::Partial => {
            write!(out, "\r  ▸ {text}")?;
        }
        SegmentKind::Final => {
            write!(out, "\n  ✓ {text}\n")?;
        }
    }
    out.flush()
}

/// Offline ASR over a WAV file. The engine (Whisper or streaming) is selected
/// automatically from the model directory's file layout.
fn cmd_transcribe(args: &[String]) -> Result<(), Box<dyn Error>> {
    let wav = flag_value(args, "--wav").ok_or("missing --wav <path>")?;
    let model_dir = flag_value(args, "--model-dir").unwrap_or(DEFAULT_MODEL_DIR);

    println!("Loading ASR model from: {model_dir}");
    let engine = asr::Engine::from_model_dir(model_dir)?;
    println!("Transcribing: {wav}");
    let transcript = engine.transcribe_wav(wav)?;
    println!("Transcript: {transcript}");
    for seg in &transcript.segments {
        println!("  [{:?}] {}", seg.kind, seg.text);
    }
    Ok(())
}

/// Live mic -> pipeline (S7). Capture and ASR each run on their own threads,
/// joined by a ring buffer; partial/final segments stream here as typed events.
/// Ctrl-C ends capture gracefully so trailing speech is flushed to a final.
fn cmd_stream(args: &[String]) -> Result<(), Box<dyn Error>> {
    let model_dir = flag_value(args, "--model-dir").unwrap_or(DEFAULT_MODEL_DIR);
    let named = flag_value(args, "--device").map(str::to_string);
    let gain: f32 = flag_value(args, "--gain")
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_GAIN);

    println!("Loading streaming ASR model from: {model_dir}");
    let source = pipeline::MicSource::new(named, gain);
    println!("Input device: {}", source.device_name());
    let mut pipe = pipeline::Pipeline::new(pipeline::PipelineConfig {
        model_dir: model_dir.into(),
        source: Box::new(source),
        ring_capacity: None,
        // Silero VAD segments the stream into clean utterances before the
        // offline Whisper engine decodes them (same routing the original
        // Roofle used). Required for the default Whisper model.
        vad_model: Some("assets/models/silero_vad.onnx".into()),
    })?;
    println!("Transcribing — speak now, Ctrl-C to stop");
    pipe.start()?;

    // Ctrl-C ends capture gracefully so the in-flight utterance is finalized.
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    ctrlc::set_handler(move || {
        let _ = stop_tx.send(());
    })
    .map_err(|e| format!("failed to install Ctrl-C handler: {e}"))?;

    let mut stdout = io::stdout();
    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }
        match pipe.events().recv_timeout(Duration::from_millis(50)) {
            Ok(seg) => print_segment(&mut stdout, seg.kind, &seg.text)?,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Flush trailing speech, then print anything shutdown produced (e.g. the
    // final segment for the in-flight utterance).
    pipe.stop();
    for seg in pipe.events().try_iter() {
        print_segment(&mut stdout, seg.kind, &seg.text)?;
    }
    println!();
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.first().map(String::as_str) {
        Some("transcribe") => return cmd_transcribe(&args[1..]),
        Some("stream") => return cmd_stream(&args[1..]),
        _ => {}
    }

    if args.iter().any(|a| a == "--list") {
        print_devices()?;
        return Ok(());
    }

    // Optional --device <name>; otherwise the default input device.
    let named = flag_value(&args, "--device").map(str::to_string);

    let (tx, rx) = std::sync::mpsc::channel::<mic::Frame>();
    let capture = match &named {
        Some(name) => {
            println!("Opening input device: {name}");
            mic::open_named(name, tx)?
        }
        None => {
            let def = mic::default_input_name().unwrap_or_else(|| "<none>".into());
            println!("Default input device: {def}");
            mic::open_default(tx)?
        }
    };
    capture.play()?;
    println!(
        "Capturing @ {} Hz, {} ch (mono downmix) — Ctrl-C to stop",
        capture.sample_rate(),
        capture.channels()
    );

    // Main thread owns the consumer: print a live level meter per frame.
    let mut stdout = io::stdout();
    for frame in rx {
        let line = format!("\r|{}|  rms {:.2}   ", meter(frame.rms), frame.rms);
        if stdout.write_all(line.as_bytes()).is_err() {
            break;
        }
        if stdout.flush().is_err() {
            break;
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("\n[roofle-cli] error: {e}");
        std::process::exit(1);
    }
}
