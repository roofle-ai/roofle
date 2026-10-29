# Developing Roofle

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
scripts/clean.sh            # DESTRUCTIVE: removes the installed app, its data, logs and permissions
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
ship the DMG and the Homebrew cask: see **[RELEASING.md](../RELEASING.md)**.
