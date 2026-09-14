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

## Storage (SQLite)

Every recording persists to a local SQLite database in the app data directory:

- macOS: `~/Library/Application Support/com.roofle.app/roofle.db`
- Windows: `%APPDATA%\com.roofle.app\roofle.db`

Schema (v2, migrated on startup via `PRAGMA user_version`):

```
sessions   one row per recording
           id, title (default empty), source (mic/system/both), model_id,
           started_at, ended_at, duration_secs, status (recording/completed),
           updated_at (last activity; advances when a session is continued)
segments   one row per finalized utterance, written in real time
           id, session_id, seq, start_secs, text, created_at
```

Finalized utterances stream into the DB as they are recognized (one write
transaction per engine poll), so a long meeting survives a crash. The essay
paragraph view is derived at read time by `render_paragraph` in
`roofle_core::storage`; paragraphs are not stored separately.

Selecting a stored transcription and pressing Play continues that session:
new speech is appended to it, its duration accumulates, and its date/time
(`updated_at`) refreshes.

Architecture: models + repository trait + `SqliteStorage` live in
`core/src/storage/` (Tauri-free, unit-tested against in-memory SQLite). The
Tauri shell opens the DB at the app data dir during setup and exposes
`list_sessions` / `get_session` / `delete_session` commands. The frontend
consumes them through the typed wrapper `src/transcripts.ts`.
