#!/usr/bin/env bash
# Download the S5/S6 development model and its bundled test WAVs.
#
# The model is a streaming Zipformer-Transducer for English:
#   sherpa-onnx-streaming-zipformer-en-20M-2023-02-17
# (int8 encoder ~43 MB + small decoder/joiner + tokens.txt + test WAVs).
# Source: the official k2-fsa/sherpa-onnx pretrained-model releases.
#
# Artifacts land under ./assets/ (gitignored — never committed):
#   assets/models/sherpa-onnx-streaming-zipformer-en-20M-2023-02-17/
#   assets/wavs/0.wav  assets/wavs/1.wav  assets/wavs/8k.wav
#
# Idempotent: skips the download if the model directory already exists.
#
# Verify S5 afterwards with:
#   cargo run -p roofle-cli -- transcribe --wav assets/wavs/0.wav
set -euo pipefail

MODEL="sherpa-onnx-streaming-zipformer-en-20M-2023-02-17"
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

mkdir -p "$WAV_DIR"
cp -f "$MODEL_DIR"/test_wavs/0.wav "$MODEL_DIR"/test_wavs/1.wav "$WAV_DIR/"

echo
echo "Test WAVs ready in $WAV_DIR"
echo "Expected transcripts (model's test_wavs/trans.txt):"
cat "$MODEL_DIR/test_wavs/trans.txt"
echo
echo "Transcribe one now with:"
echo "  cargo run -p roofle-cli -- transcribe --wav assets/wavs/0.wav"
