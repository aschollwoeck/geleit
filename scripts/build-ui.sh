#!/usr/bin/env bash
# Build the Leptos frontend (geleit-ui) to WASM and emit it into the Tauri shell's static dist/.
#
# No npm, no bundler, no Node: cargo -> wasm32 -> wasm-bindgen -> dist/pkg/. That is what keeps
# `cargo` and `deny.toml` covering the project's ENTIRE dependency tree (ADR-0012).
#
#   scripts/build-ui.sh [--release]
set -euo pipefail

cd "$(dirname "$0")/.."
PROFILE_DIR=debug
CARGO_ARGS=()
if [[ "${1:-}" == "--release" ]]; then
  PROFILE_DIR=release
  CARGO_ARGS+=(--release)
fi

OUT=crates/geleit-shell/dist/pkg
WASM=target/wasm32-unknown-unknown/$PROFILE_DIR/geleit_ui.wasm

# wasm-bindgen's CLI must EXACTLY match the wasm-bindgen crate version Leptos resolves; a mismatch
# fails at runtime with an opaque error, so check it here where the message can be useful.
CRATE_VER=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' Cargo.lock)
if [[ -z "$CRATE_VER" ]]; then
  echo "error: could not find the wasm-bindgen version in Cargo.lock" >&2
  exit 1
fi

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  cat >&2 <<EOF
error: wasm-bindgen CLI not found (need exactly $CRATE_VER).

  cargo install --locked wasm-bindgen-cli@$CRATE_VER

or grab the prebuilt binary (much faster):
  https://github.com/rustwasm/wasm-bindgen/releases/tag/$CRATE_VER
EOF
  exit 1
fi

CLI_VER=$(wasm-bindgen --version | awk '{print $2}')
if [[ "$CLI_VER" != "$CRATE_VER" ]]; then
  echo "error: wasm-bindgen CLI is $CLI_VER but the crate is $CRATE_VER — they must match." >&2
  echo "       cargo install --locked wasm-bindgen-cli@$CRATE_VER" >&2
  exit 1
fi

echo "building geleit-ui -> wasm32 ($PROFILE_DIR)"
cargo build -p geleit-ui --target wasm32-unknown-unknown "${CARGO_ARGS[@]}"

echo "wasm-bindgen $CLI_VER -> $OUT"
rm -rf "$OUT"
wasm-bindgen --target web --no-typescript --out-dir "$OUT" "$WASM"

echo "ok: $(du -h "$OUT"/geleit_ui_bg.wasm | cut -f1) wasm"
