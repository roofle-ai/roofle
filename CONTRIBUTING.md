# Contributing to Roofle

Thanks for helping. Bug reports, fixes and focused improvements are welcome.

## Setup

Requirements: stable Rust, Node 20+, Xcode command line tools, Apple Silicon Mac on macOS 13+.

```sh
npm install
scripts/fetch-assets.sh    # Whisper model + VAD + test audio, once
npm run tauri dev
```

## Before you open a PR

```sh
cargo test --workspace
npx tsc --noEmit
```

Tests that need the downloaded model skip themselves when it is absent.

## Guidelines

- Keep changes small and focused; open an issue first for anything large.
- SQLite schema changes need a **new** migration step (`PRAGMA user_version`), never an edit to an old one.
- LLM prompts live in `live-agent/prompts/` and `offline-agent/prompts/`. Changing one changes behaviour: check the matching code and run `cargo test -p live-agent`.
- Storage and pipeline code is tested against in-memory SQLite; follow that pattern.
- No secrets in the repo. Signing material and `.env*` files are gitignored.

## License

By contributing you agree your work is released under the [MIT License](LICENSE).
