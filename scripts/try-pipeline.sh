#!/usr/bin/env bash
# Testing only: run the question pipeline on a transcript with a real model
# and print the questions it would surface.
#
#   OPENROUTER_API_KEY=... scripts/try-pipeline.sh transcript.txt ["goal of the call"]
#
# Transcript: lines like `MM:SS Presenter|Other: text`, or plain lines
# (10 s apart, all "Other"). Optional: ROOFLE_MODEL (default
# openai/gpt-4o-mini; any OpenRouter model id that supports structured
# outputs), ROOFLE_BASE_URL (default https://openrouter.ai/api/v1).
set -euo pipefail

[[ $# -ge 1 && -f "$1" ]] || { echo "usage: $0 transcript.txt [goal]" >&2; exit 2; }
[[ -n "${OPENROUTER_API_KEY:-${OPENAI_API_KEY:-}}" ]] || { echo "set OPENROUTER_API_KEY" >&2; exit 2; }

# Run on a temp copy so no cache or label file is ever written next to your transcript.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cp "$1" "$tmp/t.txt"

cd "$(dirname "$0")/.."
args=("$tmp/t.txt")
[[ -n "${2:-}" ]] && args+=(--goal "$2")
cargo run -q -p live-agent --example replay -- "${args[@]}"
