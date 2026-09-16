#!/usr/bin/env bash
# Download the development ASR model and its bundled test WAVs.
#
# The model is an exported (offline) Whisper `base.en`:
#   sherpa-onnx-whisper-base.en
# (encoder + decoder + tokens.txt + test WAVs). Whisper gives much higher
# accuracy than the earlier streaming Zipformer-Transducer models, at the cost
# of being offline: core::asr buffers ~28 s windows and decodes each one.
# Source: the official k2-fsa/sherpa-onnx pretrained-model releases.
#
# Artifacts land under ./assets/ (gitignored — never committed):
#   assets/models/sherpa-onnx-whisper-base.en/
#   assets/wavs/0.wav  assets/wavs/1.wav  assets/wavs/8k.wav
#
# Idempotent: skips the download if the model directory already exists.
#
# Verify afterwards with:
#   cargo run -p roofle-cli -- transcribe --wav assets/wavs/0.wav
set -euo pipefail

MODEL="sherpa-onnx-whisper-base.en"
URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/${MODEL}.tar.bz2"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_DIR="$ROOT/assets/models/$MODEL"
WAV_DIR="$ROOT/assets/wavs"

if [[ -d "$MODEL_DIR" ]]; then
  echo "Model already present: $MODEL_DIR"
else
  echo "Downloading $MODEL ..."
  mkdir -p "$ROOT/assets/models"
  curl -L --fail -o "/tmp/${MODEL}.tar.bz2" "$URL"
  tar -xjf "/tmp/${MODEL}.tar.bz2" -C "$ROOT/assets/models"
  echo "Extracted to $MODEL_DIR"
fi

# The offline Whisper engine routes audio through a Silero VAD (segmenting the
# stream into clean speech utterances) before decoding, so fetch it too.
VAD="$ROOT/assets/models/silero_vad.onnx"
if [[ -f "$VAD" ]]; then
  echo "VAD already present: $VAD"
else
  echo "Downloading silero_vad.onnx ..."
  curl -L --fail -o "$VAD" \
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx"
fi

mkdir -p "$WAV_DIR"
cp -f "$MODEL_DIR"/test_wavs/0.wav "$MODEL_DIR"/test_wavs/1.wav "$WAV_DIR/"

echo
echo "Test WAVs ready in $WAV_DIR"
echo "Expected transcripts (model's test_wavs/trans.txt):"
cat "$MODEL_DIR/test_wavs/trans.txt"
echo
echo "Transcribe one now with:"
echo "  cargo run -p roofle-cli -- transcribe --wav assets/wavs/0.wav"
