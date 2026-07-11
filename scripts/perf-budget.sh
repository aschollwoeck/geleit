#!/usr/bin/env bash
# Enforce the constitution P4 performance budgets (M9). Fails (non-zero) if any ceiling is breached.
#
#   Binary size (stripped) ............. <= 30 MB   (always checked)
#   Cold start (exec -> first paint) ... <= 1200 ms (needs a display; skipped without $DISPLAY)
#   Idle RSS (window open) ............. <= 280 MB  (needs a display)
#
# "Ceilings, not targets" — a change that moves any of these toward its ceiling is a defect to be
# justified, not a cost to be absorbed. Always measured on a **release** build (debug is 10-50x
# slower and misleading). In CI this runs under xvfb; locally it uses your display.
#
#   scripts/perf-budget.sh
set -euo pipefail
cd "$(dirname "$0")/.."

BIN=target/release/geleit-app
BIN_CEIL_MB=30
COLD_CEIL_MS=1200
RSS_CEIL_MB=280

fail=0
say() { printf '%-34s %s\n' "$1" "$2"; }

# --- 1. Binary size (always) --------------------------------------------------------------------
if [[ ! -x "$BIN" ]]; then
  echo "error: $BIN not found — run: ./scripts/build-ui.sh --release && cargo build --release -p geleit-app" >&2
  exit 1
fi
size_mb=$(( $(stat -c%s "$BIN") / 1048576 ))
if (( size_mb > BIN_CEIL_MB )); then
  say "binary size" "${size_mb} MB  ✗ (> ${BIN_CEIL_MB} MB)"; fail=1
else
  say "binary size" "${size_mb} MB  ✓ (<= ${BIN_CEIL_MB} MB)"
fi

# --- 2 & 3. Cold start + idle RSS (need a display) ----------------------------------------------
if [[ -z "${DISPLAY:-}" ]]; then
  say "cold start / idle RSS" "SKIPPED (no \$DISPLAY)"
  [[ "$fail" -eq 0 ]] && echo "perf budget: binary OK; run under a display for cold-start/RSS." || true
  exit "$fail"
fi

# median of 3 cold starts, exec -> the GELEIT_READY marker the app prints on first page load
DB=$(mktemp --suffix=.db)
trap 'rm -f "$DB"' EXIT
cold_runs=()
last_rss=0
for _ in 1 2 3; do
  start=$(date +%s%3N)
  GELEIT_PERF=1 GELEIT_DB="$DB" "$BIN" >/tmp/geleit-perf.out 2>/dev/null &
  pid=$!
  # wait for the READY line (cap ~15s)
  for _ in $(seq 1 1500); do
    if grep -q GELEIT_READY /tmp/geleit-perf.out 2>/dev/null; then break; fi
    sleep 0.01
  done
  now=$(date +%s%3N)
  cold_runs+=( $(( now - start )) )
  sleep 1
  last_rss=$(( $(awk '/VmRSS/{print $2}' "/proc/$pid/status" 2>/dev/null || echo 0) / 1024 ))
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  sleep 0.5
done
# median of 3
IFS=$'\n' sorted=($(sort -n <<<"${cold_runs[*]}")); unset IFS
cold_ms=${sorted[1]}

if (( cold_ms > COLD_CEIL_MS )); then
  say "cold start (exec→first paint)" "${cold_ms} ms  ✗ (> ${COLD_CEIL_MS} ms)"; fail=1
else
  say "cold start (exec→first paint)" "${cold_ms} ms  ✓ (<= ${COLD_CEIL_MS} ms)"
fi
if (( last_rss > RSS_CEIL_MB )); then
  say "idle RSS" "${last_rss} MB  ✗ (> ${RSS_CEIL_MB} MB)"; fail=1
else
  say "idle RSS" "${last_rss} MB  ✓ (<= ${RSS_CEIL_MB} MB)"
fi

[[ "$fail" -eq 0 ]] && echo "perf budget: all ceilings OK." || echo "perf budget: CEILING BREACHED." >&2
exit "$fail"
