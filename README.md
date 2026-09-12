# Roofle

Cross-platform real-time audio transcription: capture microphone and/or system
audio, run streaming on-device ASR, and show a live-updating transcript.

- **Architecture & stack**: [design.md](design.md) (Rev 2)
- **Gate-by-gate build plan**: [plans/implementation-plan.md](plans/implementation-plan.md)
- **Status**: S1 — workspace scaffold (this README is a stub; will grow).

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

## Getting started (S1)

```bash
cargo build          # builds core/ + cli/
cargo run -p roofle-cli
```

Requires a Rust toolchain (stable) and, on macOS, Xcode command line tools.
