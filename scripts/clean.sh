#!/usr/bin/env bash
# DESTRUCTIVE dev tool: removes the installed Roofle app, its data, logs and caches,
# and resets its macOS permissions (TCC). Asks before doing anything.
set -euo pipefail

echo "This will delete:"
echo "  /Applications/Roofle.app"
echo "  ~/Library/Application Support/com.roofle.app  (database, models)"
echo "  ~/Library/Logs/com.roofle.app"
echo "  ~/Library/Caches/com.roofle.app"
echo "and reset Roofle's microphone and screen-recording permissions."
read -r -p "Continue? [y/N] " answer
[[ "$answer" =~ ^[Yy]$ ]] || { echo "Aborted."; exit 1; }

rm -rf "$HOME/Library/Caches/com.roofle.app"
rm -rf "$HOME/Library/Logs/com.roofle.app"
rm -rf /Applications/Roofle.app
rm -rf "$HOME/Library/Application Support/com.roofle.app"
tccutil reset All com.roofle.app
