# Roofle -- Cross-Platform Real-Time Audio Transcription App
> Revision 2 — replaces the original whisper-rs + Swift-shim approach. See "Revision history" at the bottom for what changed and why.

## Overview

Build a desktop app (macOS first, Windows later) that:
1. Captures audio from the user's **microphone** and/or **system/device audio output**
2. Runs **real-time streaming transcription** on that audio (on-device, offline)
3. Displays a live-updating transcript in a UI
4. Ships as a shared codebase with platform-specific code isolated to audio capture only

## Tech stack

| Layer | Technology | Reason |
|---|---|---|
| UI shell | Tauri 2 (Rust backend + web frontend) | Small binary, native webview, built-in JS↔Rust IPC |
| Core logic | Rust, split into a pure `core/` library crate | Single codebase, testable, reusable, no UI-shell coupling |
| Mic capture | `cpal` | Cross-platform audio I/O (CoreAudio on macOS, WASAPI on Windows) |
| Transcription | `sherpa-rs` (bindings to sherpa-onnx) | **Purpose-built for streaming ASR**, official Rust bindings, built-in VAD, runs on macOS + Windows (x64/arm64); also runs Whisper models if needed |
| macOS system-audio capture | `screencapturekit` crate (pure Rust) | Safe bindings for Apple's ScreenCaptureKit, `captures_audio(true)`, macOS 13+. **No Swift/Obj-C shim required** |
| Windows system-audio capture | WASAPI loopback via `windows-rs` | Native, pure Rust, no shim |
| Resampling / downmix | `rubato` + manual stereo→mono | Normalize capture to 16kHz mono at the pipeline boundary |

## Why these choices (rationale)

- **sherpa-rs over whisper-rs**: whisper.cpp is batch-oriented; true real-time streaming would require manually reimplementing whisper.cpp's `stream`-style overlapping-window + token-diff trick. sherpa-rs exposes streaming ASR (online Zipformer-Transducer / Paraformer / CTC) as a supported API, with built-in VAD for utterance segmentation. It is a superset — it also runs Whisper models if a Whisper accuracy profile is ever wanted. This makes the hardest milestone (real-time transcription) a supported call rather than hand-rolled logic.
- **screencapturekit crate over a Swift/Obj-C FFI shim**: same TCC permission model (Screen Recording), same audio, but pure Rust — no Swift project, no bridging headers, no FFI glue to maintain. Directly serves the "minimal platform-specific code" goal.
- **core/ library + Tauri shell split**: lets the audio→ASR pipeline be validated as a headless CLI before any UI exists (de-risks the hardest part), keeps the pipeline unit-testable, and leaves the option to swap the UI shell later without touching transcription logic.

## 1. Project structure

```
audio-transcriber/
├── core/                         # Pure-Rust library — NO Tauri dependency
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── audio/
│       │   ├── mod.rs
│       │   ├── mic.rs            # cpal mic capture (cross-platform)
│       │   ├── system_macos.rs   # screencapturekit crate (pure Rust, macOS 13+)
│       │   ├── system_windows.rs # WASAPI loopback via windows-rs
│       │   └── processing.rs     # resample (rubato) + stereo->mono + gain
│       ├── asr/
│       │   ├── mod.rs            # sherpa-rs engine + built-in VAD
│       │   └── segments.rs       # partial/final segment types
│       └── pipeline.rs           # ring buffer -> VAD -> streaming ASR -> events
├── src-tauri/                    # Thin Tauri shell wrapping core/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── src/
│       ├── main.rs               # Tauri entrypoint, registers commands
│       ├── commands.rs           # #[tauri::command]s calling into core/
│       └── state.rs              # shared app state (recording status, transcript buffer)
├── src/                          # Frontend (Svelte recommended; plain HTML/JS ok)
│   ├── index.html
│   ├── main.ts
│   └── styles.css
├── cli/                          # Headless spike binary (reuses core/)
│   └── src/main.rs
├── .github/workflows/ci.yml      # macos-latest + windows-latest matrix
├── package.json
├── Cargo.toml                    # workspace root
└── README.md
```

## Pipeline

```mermaid
flowchart LR
    A[Source select] --> B[Mic via cpal]
    A --> C[System audio]
    C --> C1[macOS screencapturekit]
    C --> C2[Windows WASAPI loopback]
    B --> P[Processing resample 16k mono]
    C1 --> P
    C2 --> P
    P --> R[Ring buffer]
    R --> V[VAD speech gate]
    V --> S[sherpa-rs streaming ASR thread]
    S --> E[Partial and final segments]
    E --> F[Frontend transcript panel]
```

## 2. Milestones (build in this order)

### Milestone 0 — Skeleton + CI
- Set up a Cargo **workspace** with `core/` (pure lib, no Tauri) + `src-tauri/` (Tauri shell) + `cli/`.
- Scaffold the Tauri app (`npm create tauri-app@latest`) and confirm it builds/runs on macOS.
- Add a GitHub Actions matrix (**macos-latest + windows-latest**) so Windows-only code compiles continuously from day one — critical because the dev machine is macOS and WASAPI code cannot otherwise be compiled locally.

### Milestone 1 — Headless CLI spike (de-risk streaming ASR)
- `cli/` binary using `core/`: cpal mic capture → resample/downmix to 16kHz mono → sherpa-rs streaming ASR → print partial/final segments to the terminal.
- Purpose: validate latency/accuracy on real audio **before** any UI work, and **pick the default model empirically** (streaming Zipformer-Transducer vs Paraformer vs Whisper; tiny/base/small variants).

### Milestone 2 — Core audio pipeline
- Implement `mic.rs` (cpal: enumerate devices, default, open input stream) and `processing.rs` (rubato resample to 16kHz + stereo→mono downmix + optional gain normalization).
- Stream raw PCM into a ring buffer; expose capture start/stop.

### Milestone 3 — Core transcription
- Implement `asr/` with sherpa-rs streaming engine + built-in VAD on a background thread/task.
- **Never block the audio callback thread** — feed the ring buffer, let the ASR thread pull and infer.
- Emit typed partial/final segment events on a channel.

### Milestone 4 — Tauri shell integration
- Wrap `core/` in `#[tauri::command]`s (start/stop, select source, set model).
- Emit transcript updates to the frontend via Tauri events (`app.emit_all("transcript-update", segment)`).

### Milestone 5 — Frontend UI
- Start/Stop recording button; **source selector (Mic / System / Both)**.
- Live-updating transcript panel (partial + final segments rendered incrementally).
- Audio level meter (RMS of captured buffer); permission status indicator (mic granted/denied).

### Milestone 6 — macOS system audio
- Use the `screencapturekit` crate: `SCStreamConfiguration` with `captures_audio(true)`, 48kHz stereo → downmix/resample into the same pipeline.
- Screen Recording permission UX (separate TCC category from mic; `NSScreenCaptureUsageDescription`). Requires macOS 13+.

### Milestone 7 — Windows system audio
- WASAPI loopback via `windows-rs`: `IMMDeviceEnumerator` → default render endpoint → `IAudioClient` with `AUDCLNT_STREAMFLAGS_LOOPBACK` → `IAudioCaptureClient`.
- Feed into the same shared pipeline. Verified continuously by CI on `windows-latest`.

### Milestone 8 — Polish
- Graceful permission-denied states with in-app instructions.
- Model manager: download on first run with progress events + checksums; tiny/base/small selection with latency/accuracy note.
- Export transcript to `.txt` / `.srt`.
- End-to-end test on both platforms.

## 3. Key implementation notes

- **Never block the real-time audio callback.** `cpal`'s callback runs on a high-priority thread — heavy work (ASR inference, VAD, disk I/O) happens on separate threads/tasks, communicating via a lock-free channel (`crossbeam-channel` or `tokio::sync::mpsc`).
- **Audio format**: ASR engines consume 16kHz mono `f32` PCM. Normalize at the capture boundary: downmix stereo→mono and resample (rubato) regardless of the source's native sample rate (mic typically 44.1/48k; ScreenCaptureKit system audio 48k).
- **VAD is a first-class component**: sherpa-onnx bundles VAD; use it for speech gating and utterance segmentation. Avoid the separate pure-Rust `silero-vad` crate — it is GPL-2.0 licensed (a distribution hazard) and unnecessary here.
- **Mic + system audio mixing / echo**: when both sources are live, system audio played through speakers re-enters the mic. For v1 expose a source selector and merge into **one transcript stream**; recommend headphones for the "Both" case. Echo cancellation is out of scope for this stack.
- **Model size vs latency**: start with the smallest streaming model for lowest latency during development; benchmark on actual target hardware. sherpa-rs also supports Whisper models if a different accuracy profile is needed.
- **Permissions**:
  - macOS mic: `NSMicrophoneUsageDescription` in Info.plist (Tauri bundle settings).
  - macOS system audio: Screen Recording permission (TCC; `NSScreenCaptureUsageDescription`), separate from mic — required even for audio-only capture via `SCStream`.
  - Windows: no equivalent prompt; handle `AccessDenied` errors gracefully.
- **Unsigned app caveat (macOS)**: until code-signed/notarized with a paid Apple Developer account, TCC permission prompts can behave inconsistently across rebuilds — reset with `tccutil reset Microphone` / `tccutil reset ScreenCapture` during development. Document in the README.
- **CI**: run `cargo check`/`cargo test` on both `macos-latest` and `windows-latest`; gate the platform-specific system-audio modules behind `#[cfg(target_os = "...")]` so each platform compiles the right one.

## 4. Dependencies checklist

**Cargo workspace (`Cargo.toml`)**
```toml
[workspace]
members = ["core", "src-tauri", "cli"]
resolver = "2"
```

**core (`core/Cargo.toml`)**
```toml
[dependencies]
cpal = "0.15"
sherpa-rs = "0.4"                 # streaming ASR + VAD (bindings to sherpa-onnx)
rubato = "0.15"                   # resampling
crossbeam-channel = "0.5"
serde = { version = "1", features = ["derive"] }

[target.'cfg(target_os = "macos")'.dependencies]
screencapturekit = "10"           # pure-Rust ScreenCaptureKit audio capture, macOS 13+

[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = ["Win32_Media_Audio", "Win32_System_Com"] }
```

> Versions are starting points; lock exact versions during M0/M1. `sherpa-rs` version should be verified at implementation time.

**Frontend**: Svelte recommended (lightweight, reactive, pairs well with Tauri events); plain HTML/JS is an acceptable starting point.

**Models**: download sherpa-onnx GGML/ONNX models from the official k2-fsa sherpa-onnx model repository. Document exact URLs and on-disk cache location; download on first run with progress + checksum (do not bundle large models inside the app binary).

## 5. Open decisions (confirm during implementation, do not assume)

- **Default model size** — decide empirically after the M1 CLI spike (streaming Zipformer-Transducer vs Paraformer vs Whisper; tiny/base/small).
- **Distribution plan** — personal use vs distributing to others; affects macOS code-signing/notarization priority and model bundling.
- **Separate tracks vs merged stream** — v1 uses one merged transcript stream; a separate-tracks UI is a future option, not required.
- **Windows test devices** — some virtual audio devices behave differently with WASAPI loopback; document targets.

## Revision history

- **Rev 2 (current)**: Replaced `whisper-rs` with `sherpa-rs` (true streaming ASR + built-in VAD); replaced the hand-rolled Swift/Obj-C shim with the pure-Rust `screencapturekit` crate; restructured into a `core/` library + Tauri shell + `cli/`; added CI matrix from day one; added VAD as a first-class component; documented the echo consideration for mic + system audio mixing.
- **Rev 1 (original)**: Tauri + cpal + whisper-rs + custom Swift shim + WASAPI loopback.
