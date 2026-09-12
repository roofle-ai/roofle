# Roofle

Cross-platform real-time audio transcription: capture microphone and/or system
audio, run streaming on-device ASR, and show a live-updating transcript.

- **Architecture & stack**: [design.md](design.md) (Rev 2)
- **Gate-by-gate build plan**: [plans/implementation-plan.md](plans/implementation-plan.md)
- **CI**: [![CI](https://github.com/<owner>/roofle/actions/workflows/ci.yml/badge.svg)](https://github.com/<owner>/roofle/actions/workflows/ci.yml)
- **Status**: S3 — mic capture via cpal (enumerate, default device, mono downmix + RMS), CLI live level meter; CI matrix (macOS + Windows) green.

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

> The CI badge URL above needs `<owner>` replaced with the actual GitHub owner
> once the repository is pushed to GitHub.
