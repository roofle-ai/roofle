# Implementation Plan — Roofle - Real-Time Audio Transcription App

**Goal:** Build a macOS-first (Windows later) desktop app that captures microphone and/or system audio and runs real-time streaming transcription with a live transcript UI.

**Approach:** Gradual, gate-by-gate development. Every slice is an independently verifiable, runnable increment. No big-bang build — the project stays buildable and testable at every step.

**Stack (Rev 2, see `design.md`):** Tauri 2 + Rust, `core/` library (no Tauri dep) + `src-tauri/` shell + `cli/`, `cpal` (mic), `sherpa-rs` (streaming ASR + VAD), `screencapturekit` crate (macOS system audio, pure Rust), `windows-rs` (WASAPI loopback), `rubato` (resample).

## Guiding principles

1. Each slice leaves the project **buildable and testable**; if a slice can't be verified independently, split it further.
2. The transcription pipeline is **proven headlessly first** (S1–S7) *before* any Tauri/UI work (S8+). UI bugs must never mask audio/ASR bugs.
3. Tauri is a **thin shell added late** around the already-working `core/` library.
4. Platform-specific work is isolated behind `cfg(target_os = "...")`: macOS system audio (S11) and Windows WASAPI (S12) each land independently and are verified by CI.
5. Every slice has an explicit **verification gate** (DoD) that must pass before the next starts.

## Slice map

| Slice | Deliverable | Verification gate (DoD) |
|---|---|---|
| S1 | Cargo workspace scaffold (`core` + `cli`), `.gitignore`, README stub | `cargo build` green |
| S2 | CI matrix — macos-latest + windows-latest, `cargo check --workspace` | workflow added; check passes on both platforms |
| S3 | Mic capture spike (`core::audio::mic` via cpal: enumerate, default device, 16 kHz mono stream to channel, RMS levels); `cli` prints live levels | live level output; mic permission prompt appears on macOS |
| S4 | Audio processing (`core::audio::processing`: rubato resample to 16 kHz + stereo→mono downmix + optional gain) | unit tests green (synthetic buffers) |
| S5 | ASR engine offline test (`core::asr` with sherpa-rs: load model, transcribe a known test WAV) | correct transcript output — isolates sherpa-rs integration risk |
| S6 | **M1 headless CLI spike** — wire mic → processing → sherpa-rs streaming ASR → print partial/final segments live | real-time partial text appears in terminal; pick default model empirically and record choice in `design.md` |
| S7 | Pipeline refactor — `core::pipeline` (ring buffer, background ASR thread that never touches the audio callback, typed partial/final segment events, `Source` trait) | CLI still works via pipeline API; tests green |
| S8 | Tauri scaffold (`npm create tauri-app`), add `src-tauri` to workspace, placeholder `ping` command | `tauri dev` runs; IPC works |
| S9 | **M4 shell integration** — `src-tauri` commands wrap core pipeline (start/stop/source), emit `transcript-update` events | transcript events arrive (verified from a temporary UI stub) |
| S10a | Static UI — start/stop, source selector (Mic/System/Both), transcript panel, level meter, permission status | renders |
| S10b | Live UI wiring — bind events to transcript panel + level meter + permission status | end-to-end with mic on macOS |
| S11 | **M6 macOS system audio** — `screencapturekit` crate: `SCStreamConfiguration` `captures_audio(true)` → downmix/resample → same pipeline; Screen Recording permission UX | transcribes system audio on macOS |
| S12 | **M7 Windows system audio** — WASAPI loopback via `windows-rs` behind `cfg(target_os = "windows")` | CI compiles it on windows-latest; runtime test deferred to a Windows machine |
| S13 | **M8 polish** — permission-denied UX, model manager (download on first run + progress + checksums + model choice), export `.txt`/`.srt` | end-to-end on both platforms |

## Notes

- **Never block the audio callback.** ASR inference/VAD run on a separate thread fed by a ring buffer.
- **Audio format boundary:** normalize all sources to 16 kHz mono `f32` (downmix + resample) in `core::audio::processing`.
- **VAD** is first-class via sherpa-onnx; avoid the GPL-2.0 `silero-vad` crate.
- **Echo:** when mic + system audio are both live, recommend headphones; merged single transcript stream for v1.
- **macOS permissions:** mic (`NSMicrophoneUsageDescription`) vs system audio (Screen Recording / `NSScreenCaptureUsageDescription`) are separate TCC categories.
- **Unsigned-app caveat:** until code-signed/notarized, reset TCC with `tccutil reset Microphone` / `tccutil reset ScreenCapture` during development.

## Open decisions (confirm as implementation proceeds)

- Default model size — decided empirically after S6 (S6 records the choice in `design.md`).
- Distribution plan (personal vs public) — drives macOS code-signing/notarization priority in S13.
- Separate-tracks UI vs merged stream — v1 uses merged stream; separate tracks is a future option.
- Windows test devices — some virtual audio devices behave differently with WASAPI loopback; document targets.

## Repository layout (target)

```
audio-transcriber/
├── core/                 # pure-Rust lib (audio + processing + ASR + pipeline)
├── src-tauri/            # Tauri shell (added S8+)
├── src/                  # frontend (S10+)
├── cli/                  # headless spike binary
├── .github/workflows/ci.yml
├── Cargo.toml            # workspace root
└── design.md             # architecture + stack (Rev 2)
```
