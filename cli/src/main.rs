//! roofle-cli — headless spike binary.
//!
//! S3: microphone capture via `cpal` with live RMS level output.
//!   - default: open the system-default input device and print a level meter
//!   - `--list`: enumerate input devices and exit
//!   - `--device <name>`: open a specific input device
//!
//! Later slices add processing (S4) and streaming ASR (S5–S6).

use std::io::Write;
use std::sync::mpsc;

use roofle_core::audio::mic;

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

fn run() -> Result<(), mic::CaptureError> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--list") {
        print_devices()?;
        return Ok(());
    }

    // Optional --device <name>; otherwise the default input device.
    let named = args
        .windows(2)
        .find(|w| w[0] == "--device")
        .map(|w| w[1].clone());

    let (tx, rx) = mpsc::channel::<mic::Frame>();
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
    let mut stdout = std::io::stdout();
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
        eprintln!("\n[mic] error: {e}");
        std::process::exit(1);
    }
}
