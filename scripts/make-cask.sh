#!/usr/bin/env bash
# Generate the Homebrew cask for a built Roofle DMG.
#
# Reads the version from src-tauri/tauri.conf.json, hashes the DMG and writes
# packaging/homebrew/Casks/roofle.rb from packaging/homebrew/roofle.rb.tmpl.
# Copy that file into your tap repo (homebrew-roofle/Casks/roofle.rb) and push.
#
# Usage:
#   ROOFLE_URL_PREFIX=https://github.com/<owner>/roofle/releases/download \
#   ROOFLE_HOMEPAGE=https://github.com/<owner>/roofle \
#     scripts/make-cask.sh [path/to/Roofle_<version>_aarch64.dmg]
#
# The DMG defaults to target/release/bundle/dmg/Roofle_<version>_aarch64.dmg.
# The cask expects it at "$ROOFLE_URL_PREFIX/v<version>/Roofle_<version>_aarch64.dmg".
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(sed -n 's/^[[:space:]]*"version":[[:space:]]*"\([^"]*\)".*/\1/p' "$ROOT/src-tauri/tauri.conf.json" | head -1)"
DMG="${1:-$ROOT/target/release/bundle/dmg/Roofle_${VERSION}_aarch64.dmg}"
URL_PREFIX="${ROOFLE_URL_PREFIX:-https://github.com/OWNER/roofle/releases/download}"
HOMEPAGE="${ROOFLE_HOMEPAGE:-https://github.com/OWNER/roofle}"
OUT="$ROOT/packaging/homebrew/Casks/roofle.rb"

[[ -n "$VERSION" ]] || { echo "could not read version from tauri.conf.json" >&2; exit 1; }
[[ -f "$DMG" ]] || { echo "DMG not found: $DMG (build it with: npm run tauri build)" >&2; exit 1; }
if [[ "$URL_PREFIX$HOMEPAGE" == *OWNER* ]]; then
  echo "warning: ROOFLE_URL_PREFIX / ROOFLE_HOMEPAGE not set; the cask still contains OWNER" >&2
fi

SHA256="$(shasum -a 256 "$DMG" | cut -d' ' -f1)"
mkdir -p "$(dirname "$OUT")"
sed -e "s|@VERSION@|$VERSION|" -e "s|@SHA256@|$SHA256|" \
    -e "s|@URL_PREFIX@|${URL_PREFIX%/}|" -e "s|@HOMEPAGE@|$HOMEPAGE|" \
    "$ROOT/packaging/homebrew/roofle.rb.tmpl" > "$OUT"

echo "wrote $OUT"
echo "  version $VERSION"
echo "  sha256  $SHA256"
echo "Upload $(basename "$DMG") to the v$VERSION release, then copy the cask to your tap's Casks/ folder."
