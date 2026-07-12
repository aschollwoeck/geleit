#!/usr/bin/env bash
# Reclaim disk from Cargo's `target/` directory.
#
# Cargo never garbage-collects `target/`: across a long-lived project it piles up artifacts from
# superseded compiles, old toolchains, and incremental-compilation caches, and can reach tens of GB.
# This prunes the stale parts with `cargo-sweep` — without a full `cargo clean`, which would throw
# away *everything* and force a from-scratch rebuild.
#
#   scripts/sweep.sh                 # default: drop old-toolchain artifacts + anything untouched 10+ days
#   scripts/sweep.sh --time 3        # instead drop anything untouched 3+ days (more aggressive)
#   scripts/sweep.sh --maxsize 15GB  # cap target/ at a size (removes least-recently-used until under it)
#   scripts/sweep.sh --deep          # the above PLUS wipe the incremental-compilation caches
#   scripts/sweep.sh --dry-run       # show what the default would remove; delete nothing
#
# `--deep` also removes `target/**/incremental` — a pure speed-up cache Cargo regenerates on the next
# build. It's always safe (never affects a build's correctness) and is usually where the space hides
# after a burst of rebuilds, but it makes the next build a little slower. cargo-sweep alone won't touch
# a *recent* incremental cache, so reach for `--deep` when age/size sweeps don't reclaim enough.
#
# One-time install (if missing):  cargo install --locked cargo-sweep
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v cargo-sweep >/dev/null 2>&1; then
  echo "error: cargo-sweep not found. Install it once with:  cargo install --locked cargo-sweep" >&2
  exit 1
fi

before=$(du -sh . 2>/dev/null | cut -f1 || echo "?")

# `-r` sweeps every Cargo project under the repo — the main `target/` plus the throwaway `spikes/*`,
# each of which keeps its own multi-GB `target/`.
case "${1:-}" in
  --maxsize)
    cargo sweep -r --maxsize "${2:?usage: sweep.sh --maxsize <SIZE, e.g. 15GB>}"
    ;;
  --time)
    cargo sweep -r --time "${2:?usage: sweep.sh --time <DAYS>}"
    ;;
  --deep)
    cargo sweep -r --installed
    cargo sweep -r --time 10
    # Incremental caches are pure speed-up state — safe to wipe, regenerated on the next build.
    find . -type d -name incremental -prune -exec rm -rf {} + 2>/dev/null || true
    ;;
  --dry-run)
    # Preview the default: old-toolchain artifacts, then anything untouched 10+ days.
    cargo sweep -r --dry-run --installed
    cargo sweep -r --dry-run --time 10
    ;;
  "")
    cargo sweep -r --installed   # artifacts built by toolchains no longer installed
    cargo sweep -r --time 10     # anything not touched in the last 10 days
    ;;
  *)
    echo "error: unknown option '$1' (use --time N, --maxsize SIZE, --deep, or --dry-run)" >&2
    exit 2
    ;;
esac

echo "project: ${before} -> $(du -sh . 2>/dev/null | cut -f1 || echo '?')"
