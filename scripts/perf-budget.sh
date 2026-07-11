#!/usr/bin/env bash
# Enforce the constitution P4 performance budgets (M9). Fails (non-zero) if any ceiling is breached.
#
#   Binary size (stripped) ............. <= 30 MB   (always checked)
#   Cold start (exec -> first paint) ... <= 1200 ms (needs a display; skipped without $DISPLAY)
#   Idle RSS (PSS, process tree) ........ <= 280 MB  (needs a display)
#
# "Ceilings, not targets" — a change that moves any of these toward its ceiling is a defect to be
# justified, not a cost to be absorbed. Always measured on a **release** build (debug is 10-50x
# slower and misleading). In CI this runs under xvfb; locally it uses your display.
#
#   scripts/perf-budget.sh
set -uo pipefail   # NOT -e: the checks below inspect exit codes deliberately.
cd "$(dirname "$0")/.."

BIN=target/release/geleit-app
MIB=1048576
BIN_CEIL=$(( 30 * MIB ))   # bytes — compared exactly, no integer-MB truncation
COLD_CEIL_MS=1200
RSS_CEIL_MB=280
RUNS=3
WAIT_TICKS=1500            # 15 s cap per run (× sleep 0.01)

fail=0
say() { printf '%-34s %s\n' "$1" "$2"; }

# --- 1. Binary size (always, byte-precise) ------------------------------------------------------
if [[ ! -x "$BIN" ]]; then
  echo "error: $BIN not found — run: ./scripts/build-ui.sh --release && cargo build --release -p geleit-app" >&2
  exit 1
fi
size_bytes=$(stat -c%s "$BIN")
size_mb=$(awk "BEGIN{printf \"%.1f\", $size_bytes/$MIB}")
if (( size_bytes > BIN_CEIL )); then
  say "binary size" "${size_mb} MB  ✗ (> 30 MB)"; fail=1
else
  say "binary size" "${size_mb} MB  ✓ (<= 30 MB)"
fi

# --- 2 & 3. Cold start + idle RSS (need a display) ----------------------------------------------
if [[ -z "${DISPLAY:-}" ]]; then
  say "cold start / idle RSS" "SKIPPED (no \$DISPLAY)"
  exit "$fail"
fi

# WebKitGTK will not render on a headless X server (xvfb) with its default GPU/DMABUF path — it hangs
# and first paint never happens. These make it use the software path. No effect on a real display.
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export LIBGL_ALWAYS_SOFTWARE=1
export GDK_BACKEND=x11

# Sum **PSS** (KB) across a process and ALL its descendants. WebKitGTK is multi-process — the UI, the
# network process, and the web-content process (where a memory regression most likely lands) are
# separate pids, so parent-only misses most of it. We use PSS, not RSS: the three processes share
# WebKit's large libraries, and summing RSS would count those shared pages ~3× (349 MB vs the true
# ~135 MB). PSS attributes shared pages proportionally — the honest "how much memory does this app
# use" number. A dead root pid returns empty → the caller treats that as a failure, never 0.
tree_pss_kb() {
  local root=$1 pids=("$1") i=0
  while (( i < ${#pids[@]} )); do
    local kids
    kids=$(pgrep -P "${pids[$i]}" 2>/dev/null || true)
    for k in $kids; do pids+=("$k"); done
    ((i++))
  done
  local total="" got=0
  for p in "${pids[@]}"; do
    local r
    # Pss from smaps_rollup; fall back to VmRSS on a kernel that lacks it.
    r=$(awk '/^Pss:/{s+=$2} END{if(NR)print s}' "/proc/$p/smaps_rollup" 2>/dev/null || true)
    [[ -z "$r" ]] && r=$(awk '/^VmRSS:/{print $2}' "/proc/$p/status" 2>/dev/null || true)
    if [[ -n "$r" ]]; then total=$(( ${total:-0} + r )); got=1; fi
  done
  (( got )) && echo "$total" || echo ""   # empty = the tree was already gone
}

cold_runs=()
rss_runs=()
timed_out=0
DB=$(mktemp --suffix=.db)
OUT=$(mktemp)
trap 'rm -f "$DB" "$OUT"' EXIT

for _ in $(seq 1 "$RUNS"); do
  : > "$OUT"
  start=$(date +%s%3N)
  setsid env GELEIT_PERF=1 GELEIT_DB="$DB" "$BIN" >"$OUT" 2>/dev/null &
  pid=$!
  ready=0
  for _ in $(seq 1 "$WAIT_TICKS"); do
    if grep -q GELEIT_READY "$OUT" 2>/dev/null; then ready=1; break; fi
    kill -0 "$pid" 2>/dev/null || break   # the app died before first paint
    sleep 0.01
  done
  now=$(date +%s%3N)
  if (( ready )); then
    cold_runs+=( $(( now - start )) )
  else
    cold_runs+=( 999999 ); timed_out=1   # never painted (hung or crashed) — a hard failure
  fi
  # measure RSS while it's still up
  sleep 1
  rss_kb=$(tree_pss_kb "$pid")
  if [[ -n "$rss_kb" ]]; then
    rss_runs+=( $(( rss_kb / 1024 )) )
  else
    rss_runs+=( 999999 ); timed_out=1     # process gone at measure time — treat as failure, not 0
  fi
  kill "$pid" 2>/dev/null || true
  pkill -P "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  sleep 0.5
done

median() { printf '%s\n' "$@" | sort -n | awk '{a[NR]=$1} END{print a[int((NR+1)/2)]}'; }
cold_ms=$(median "${cold_runs[@]}")
rss_mb=$(median "${rss_runs[@]}")

# A run that never reached first paint (or whose process vanished) is a hard failure even if the
# median looks fine — an intermittent boot hang must not be masked by two good runs.
if (( timed_out )); then
  say "cold start / RSS" "✗ a run never reached first paint (or the process vanished)"; fail=1
fi
if (( cold_ms > COLD_CEIL_MS )); then
  say "cold start (exec→first paint)" "${cold_ms} ms  ✗ (> ${COLD_CEIL_MS} ms)"; fail=1
else
  say "cold start (exec→first paint)" "${cold_ms} ms  ✓ (<= ${COLD_CEIL_MS} ms)"
fi
if (( rss_mb > RSS_CEIL_MB )); then
  say "idle RSS (PSS, process tree)" "${rss_mb} MB  ✗ (> ${RSS_CEIL_MB} MB)"; fail=1
else
  say "idle RSS (PSS, process tree)" "${rss_mb} MB  ✓ (<= ${RSS_CEIL_MB} MB)"
fi

(( fail == 0 )) && echo "perf budget: all ceilings OK." || echo "perf budget: CEILING BREACHED." >&2
exit "$fail"
