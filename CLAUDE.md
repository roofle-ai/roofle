# CLAUDE.md

Roofle: a macOS (Apple Silicon, 13+) menu-bar app that transcribes meetings on-device (mic + system audio, Whisper via sherpa-onnx) and runs LLM agents for live notes, suggested questions, recaps and post-session review. Tauri 2 shell, Rust workspace, TypeScript/Vite/Tailwind frontend.

## Layout

- `core/` – audio capture, ASR, paragraphing, SQLite storage (`core/src/storage/`)
- `live-agent/` – live LLM agents (notes writer/validator, questions pipeline, recap, figures) and their prompts in `live-agent/prompts/`
- `offline-agent/` – post-session review + title generation; prompts in `offline-agent/prompts/`
- `call-detect/` – detects when a call starts
- `cli/` – headless binary exercising `core/` without the UI
- `src-tauri/` – Tauri shell: `engine/` (session pipeline), `agents/` and `ai/` (LLM clients), `commands/` (IPC), `app/` (tray, window, lifecycle), `models/` (model downloads)
- `src/` – frontend (`features/`, `ui/`, `app/`, `lib/`); talks to Rust only through Tauri commands/events
- `scripts/`, `packaging/` – asset fetch, DB reset, Homebrew cask

## Commands

```sh
npm install
scripts/fetch-assets.sh      # Whisper model, VAD, test wavs; once (assets/ downloads are gitignored). Model-dependent tests skip without it
npm run tauri dev            # run the app (Dock and tray icons)
cargo test --workspace       # Rust tests
npx tsc --noEmit             # frontend type check
cargo run -p roofle-cli -- --list
scripts/reset-db.sh          # wipe local DB
scripts/clean.sh             # DESTRUCTIVE (asks first): removes /Applications/Roofle.app, app data/logs/caches, resets TCC permissions
```

`ROOFLE_LOG_AI=1` mirrors transcripts and agent steps to local log files.

## Conventions and gotchas

- Version lives in the workspace `Cargo.toml` (`1.0.0-beta`) plus `package.json` and `tauri.conf.json`; keep them in sync when bumping.
- SQLite schema is migrated on startup via `PRAGMA user_version` (currently v4). Schema changes need a new migration step, never an edit to an old one.
- LLM prompts are plain `.txt` files, compiled/loaded by the agent crates. Changing a prompt changes behaviour; check the matching code in `live-agent/src/notes.rs` etc. and run `cargo test -p live-agent`.
- The LLM API key is user-supplied in Settings; no secrets in the repo (`.p12`, `.cer`, `.p8`, `.env*` are gitignored). Signing/notarization: see `RELEASING.md`.
- Open source under MIT. There is no license gate or activation anywhere in the app; do not add one without asking.
- Roofle has both a Dock icon and a menu-bar icon. Closing the window only hides it; clicking the Dock icon (or opening the app again) shows it. Quit via the tray menu, Dock → Quit or ⌘Q (all go through `lifecycle::quit`).
- Storage and pipeline code is unit-tested against in-memory SQLite; follow that pattern for new storage code.
- Test audio (`call_audio.*`) is for local pipeline experiments (`scripts/try-pipeline.sh`).
