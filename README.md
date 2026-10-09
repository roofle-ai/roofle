# Roofle

A macOS menu-bar app that listens to your meetings and conversations (your
microphone and the system audio), transcribes them **on-device**, and helps you
while they happen:

- **Live transcription** from the mic and system audio, tagged by source.
- **Live notes**: one short, continuously rewritten notes document
  (summary, key points, decisions, action items).
- **Suggested questions**: a gap or vague point worth asking about, with a
  **Source** button that jumps to the exact part of the transcript it came from.
- **Recap** of the last 30 seconds, and key figures and dates on demand.
- **Conversation review**: an offline self-assessment once a session ends.

Speech recognition (Whisper via sherpa-onnx) and storage run locally. Notes,
questions and reviews use an LLM through an API key you provide in Settings.

## Install

```sh
brew install --cask <owner>/roofle/roofle
```

or download the DMG from the releases page. Requires an Apple Silicon Mac on
macOS 13 or newer.

On first use macOS asks for **Microphone** and **Screen & System Audio
Recording** access; both are needed to capture each side of a conversation.

## Using Roofle

Roofle runs from the menu bar and launches hidden (no Dock icon). Nothing is
recorded until you start it.

- **Left-click** the tray icon to open the window.
- **Right-click** for the menu: New conversation (⌘⇧S, asks for the goal first), Stop conversation (⌘⇧T), Quit.
- Closing the window only hides it; Roofle keeps running until you choose Quit.

## Repository layout

```
roofle/
├── core/          # Rust lib: audio capture, ASR, paragraphing, SQLite storage
├── live-agent/    # Rust lib: live LLM agents (notes, suggested questions, recap) and their prompts
├── offline-agent/ # Rust lib: post-session review and title generation
├── cli/           # headless binary that exercises core/ without the UI
├── src-tauri/     # Tauri shell: engine, commands, tray, app icons, bundle config
├── src/           # TypeScript frontend (Vite + Tailwind)
├── scripts/       # fetch-assets.sh, reset-db.sh, make-cask.sh
├── packaging/     # Homebrew cask template
└── assets/        # models and test audio (downloaded, gitignored), icon sources
```

Architecture and decisions are in [design.md](design.md).

## Development

Requirements: stable Rust, Node 20+, and the Xcode command line tools.

```sh
npm install
scripts/fetch-assets.sh     # downloads the Whisper model and VAD (once)
npm run tauri dev           # runs the menu-bar app; look for the tray icon
```

Useful commands:

```sh
cargo test --workspace      # Rust tests
npx tsc --noEmit            # frontend type check
cargo run -p roofle-cli -- --list     # list audio input devices
scripts/reset-db.sh         # wipe the local database
```

Set `ROOFLE_LOG_AI=1` to mirror transcripts and agent steps to local log files
while developing.

## Data and storage

Everything is stored locally in SQLite:

- macOS: `~/Library/Application Support/com.roofle.app/roofle.db`

The schema is migrated on startup (`PRAGMA user_version`, currently v4). Main
tables: `sessions`, `audio_segments` (raw ASR utterances, linked to the log entry
they were folded into via `log_seq`), `paragraphs`, `transcript_log` (the merged,
ordered transcript the LLM agents read), `live_notes`, `questions` (each with the
`transcript_log` entry it came from) and `session_analysis`. Storage code lives
in `core/src/storage/` and is unit-tested against in-memory SQLite.

## Releasing

Building, signing and notarizing, where the Apple keys are kept, and how to
ship the DMG and the Homebrew cask: see **[RELEASING.md](RELEASING.md)**.
