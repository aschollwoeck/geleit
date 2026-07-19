#!/usr/bin/env bash
# Build + package GeleitMail for Linux (x86_64) into a release tarball (APP-5, S8.4).
# Usage: packaging/package-linux.sh [version]   (version defaults to `git describe`)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="${1:-$(git describe --tags --always 2>/dev/null || echo dev)}"
NAME="geleit-${VERSION}-linux-x86_64"

# Build the Leptos frontend to WASM first — the Tauri app embeds crates/geleit-app/dist/ (incl. the
# generated dist/pkg/, which is gitignored) at compile time, so it MUST exist before the app build or
# the packaged binary ships a blank window.
./scripts/build-ui.sh --release
cargo build --release -p geleit-app
# The web host (ADR-0014): same WASM UI over HTTP. Serves `dist/` + `web/`, which we bundle beside the
# binary — `geleit-server` finds them next to itself, so the package runs standalone.
cargo build --release -p geleit-server

DIST="$ROOT/dist"
STAGE="$DIST/$NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp target/release/geleit-app "$STAGE/"
cp target/release/geleit-server "$STAGE/"
cp packaging/geleit.desktop "$STAGE/"
cp README.md LICENSE "$STAGE/"
# Web-host runtime assets: the built WASM UI + the web shim. `geleit-server` resolves these relative to
# its own location (see `asset_dir`), so `./geleit-server` just works from the unpacked directory.
cp -r crates/geleit-app/dist "$STAGE/dist"
cp -r crates/geleit-server/web "$STAGE/web"
tar -C "$DIST" -czf "$DIST/$NAME.tar.gz" "$NAME"
echo "packaged: dist/$NAME.tar.gz ($(du -h "$DIST/$NAME.tar.gz" | cut -f1))"
echo "  desktop:  ./geleit-app"
echo "  web host: ./geleit-server   (http://127.0.0.1:8080)"
