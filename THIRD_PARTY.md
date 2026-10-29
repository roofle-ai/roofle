# Third-party notices

Roofle is released under the [MIT License](LICENSE). It builds on the open-source
software and models listed below, each of which keeps its own license. This file
covers direct dependencies and downloaded models; the full transitive lists are
in `Cargo.lock` and `package-lock.json`.

Last generated from `Cargo.lock` and `package-lock.json` at version `1.0.0-beta`.

## Speech models (downloaded on first run, not bundled in the app)

| Component | Used for | License |
|---|---|---|
| [OpenAI Whisper](https://github.com/openai/whisper) weights (`whisper-base.en`, `whisper-small.en`), as ONNX exports published by sherpa-onnx | Speech recognition | MIT |
| [Silero VAD](https://github.com/snakers4/silero-vad) (`silero_vad.onnx`) | Voice activity detection | MIT |

Both are downloaded from the [sherpa-onnx releases](https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models)
page when you first run Roofle. The licenses above are the upstream projects'
published licenses; re-check them when you change models.

## Runtime and inference

| Component | License |
|---|---|
| [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) 1.13 (Rust bindings and ONNX Runtime-based inference) | Apache-2.0 |

## Rust dependencies (direct)

| Crate | Version | License |
|---|---|---|
| block2 | 0.6 | MIT |
| bzip2 | 0.4 / 0.5 | MIT OR Apache-2.0 |
| chrono | 0.4 | MIT OR Apache-2.0 |
| cpal | 0.15 | Apache-2.0 |
| ctrlc | 3.5 | MIT OR Apache-2.0 |
| hex | 0.4 | MIT OR Apache-2.0 |
| objc2, objc2-foundation | 0.6 / 0.3 | MIT |
| objc2-avf-audio, objc2-core-audio-types, objc2-user-notifications | 0.3 | Zlib OR Apache-2.0 OR MIT |
| rubato | 0.15 | MIT |
| rusqlite | 0.32 | MIT |
| screencapturekit | 1.5 | MIT OR Apache-2.0 |
| serde, serde_json | 1.0 | MIT OR Apache-2.0 |
| sha2 | 0.10 | MIT OR Apache-2.0 |
| sysinfo | 0.33 | MIT |
| tar | 0.4 | MIT OR Apache-2.0 |
| tauri, tauri-build | 2.x | Apache-2.0 OR MIT |
| tauri-plugin-notification, -opener, -process, -updater, -window-state | 2.x | Apache-2.0 OR MIT |
| ureq | 2.12 | MIT OR Apache-2.0 |

Across all ~590 transitive Rust crates, the licenses are permissive (MIT,
Apache-2.0, BSD, ISC, Zlib, Unicode, Unlicense/CC0), with these notes:

- **MPL-2.0** (5 crates): file-level copyleft. Used unmodified as dependencies, so it adds no obligations beyond keeping their notices.
- **LGPL-2.1-or-later** appears only inside a dual license (`MIT OR Apache-2.0 OR LGPL-2.1-or-later`, 2 crates); Roofle uses them under MIT/Apache-2.0.
- **CDLA-Permissive-2.0** (3 crates): permissive terms for bundled data.
- No GPL or AGPL dependencies were found.

## JavaScript dependencies (direct)

| Package | Version | License |
|---|---|---|
| @tauri-apps/api | 2.x | Apache-2.0 OR MIT |
| @tauri-apps/plugin-opener, -process, -updater | 2.x | MIT OR Apache-2.0 |
| @tailwindcss/vite, tailwindcss | 4.x | MIT |
| dompurify | 3.4 | MPL-2.0 OR Apache-2.0 |
| lucide | 1.x | ISC |
| marked | 18.x | MIT |
| @fontsource-variable/inter | 5.x | OFL-1.1 |
| @fontsource-variable/newsreader | 5.x | OFL-1.1 |

Build-time only: `@tauri-apps/cli` (Apache-2.0 OR MIT), `typescript` (Apache-2.0),
`vite` (MIT), `@types/node` (MIT).

## Fonts

Inter and Newsreader are bundled in the app through Fontsource and are licensed
under the [SIL Open Font License 1.1](https://openfontlicense.org). The fonts
may be bundled and redistributed with software, but are not sold on their own.

## Regenerating this list

```sh
cargo install cargo-deny && cargo deny check licenses      # Rust: fails on disallowed licenses
cargo metadata --format-version 1 --locked                 # Rust: raw license data
npx license-checker --summary                              # JavaScript
```

When you add a dependency or change a model, update this file.
