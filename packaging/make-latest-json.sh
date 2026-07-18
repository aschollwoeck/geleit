#!/usr/bin/env bash
# Build the updater feed `latest.json` (APP-7, ADR-0013) from the signed AppImage produced by
# `cargo tauri build --bundles appimage`. The in-app updater reads this file to learn the newest
# version, its signature, and where to download it. Called by release.yml after the signed build.
#
#   packaging/make-latest-json.sh v0.1.3      # tag → 0.1.3
#
# Emits ./latest.json in the repo root, referencing the GitHub Release asset for this tag.
set -euo pipefail
cd "$(dirname "$0")/.."

TAG="${1:?usage: make-latest-json.sh <vX.Y.Z tag>}"
VERSION="${TAG#v}" # strip a leading 'v'
REPO="aschollwoeck/geleit"

BUNDLE_DIR="target/release/bundle/appimage"
APPIMAGE="$(ls "$BUNDLE_DIR"/*.AppImage 2>/dev/null | head -1 || true)"
SIG="$(ls "$BUNDLE_DIR"/*.AppImage.sig 2>/dev/null | head -1 || true)"

if [[ -z "$APPIMAGE" || -z "$SIG" ]]; then
  echo "error: no signed AppImage/.sig in $BUNDLE_DIR — did the signed build run?" >&2
  exit 1
fi

NAME="$(basename "$APPIMAGE")"
SIGNATURE="$(cat "$SIG")"
URL="https://github.com/$REPO/releases/download/$TAG/$NAME"
PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# Tauri's updater keys platforms by "<os>-<arch>" (linux-x86_64 here). Add more entries when
# macOS/Windows updater artifacts are built (S8.4 follow-up).
cat > latest.json <<JSON
{
  "version": "$VERSION",
  "pub_date": "$PUB_DATE",
  "platforms": {
    "linux-x86_64": {
      "signature": "$SIGNATURE",
      "url": "$URL"
    }
  }
}
JSON

echo "wrote latest.json for $VERSION ($NAME)"
