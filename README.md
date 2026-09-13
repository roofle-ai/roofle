# Roofle

Cross-platform real-time audio transcription: capture microphone and/or system
audio, run streaming on-device ASR, and show a live-updating transcript.

- **Architecture & stack**: [design.md](design.md) (Rev 2)
- **Gate-by-gate build plan**: [plans/implementation-plan.md](plans/implementation-plan.md)
- **CI**: [![CI](https://github.com/<owner>/roofle/actions/workflows/ci.yml/badge.svg)](https://github.com/<owner>/roofle/actions/workflows/ci.yml)
- **Status**: S4 — audio processing (`core::audio::processing`: rubato resample to 16 kHz mono + downmix + gain), unit-tested; CI matrix (macOS + Windows) green.

## Repository layout

```
roofle/
├── core/        # pure-Rust lib: audio capture + processing + streaming ASR + pipeline
├── cli/         # headless spike binary that exercises core/ before any UI exists
├── src-tauri/   # Tauri shell (added in a later slice, S8+)
├── src/         # frontend (S10+)
├── plans/       # design + implementation plan
└── design.md
```

## Getting started (S3)

```bash
cargo build          # builds core/ + cli/
cargo run -p roofle-cli          # live mic level meter
cargo run -p roofle-cli -- --list # enumerate input devices
```

Requires a Rust toolchain (stable) and, on macOS, Xcode command line tools.

## Menu bar mode (S14, macOS)

Roofle runs as a **menu bar app**: it launches hidden (no Dock icon) and shows a
tray icon in the macOS menu bar. Recording is strictly **on-demand** — nothing is
captured until you choose Start.

- **Left-click** the tray icon → opens the transcript popup window.
- **Right-click** the tray icon → menu:
  - **Start Recording** (⌘⇧S) — begins capturing/transcribing on the
    last-selected source (Mic by default; change the source in the popup).
  - **Stop Recording** (⌘⇧T) — ends capture and flushes trailing speech.
  - **Quit Roofle** — exits the app.
- Closing the popup window only **hides** it; the app keeps running in the menu
  bar until you choose Quit.

> Dev note: `npm run tauri dev` now launches the hidden menu-bar app. Look for
> the Roofle tray icon at the top-right of the menu bar.

> The CI badge URL above needs `<owner>` replaced with the actual GitHub owner
> once the repository is pushed to GitHub.
