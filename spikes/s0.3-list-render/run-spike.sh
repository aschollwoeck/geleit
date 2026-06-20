#!/usr/bin/env bash
# Measure the S0.3 Slint list spike (RELEASE build) while scrolling through the WHOLE list.
# Metrics: FPS (Slint perf counter) + max RSS + deepest row actually rendered.
# - GPU runs use refresh_lazy: FPS reflects REAL scroll-driven redraws (vsync-capped ~60Hz),
#   not forced repaints — so a sustained ~60 proves real smooth motion.
# - 1k vs 50k at parity (and equal deepest-row coverage) ⇒ virtualization.
# - A software-renderer run with refresh_full_speed gives uncapped compute throughput (headroom).
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HERE/target/release/s0_3_list_render"
EVID="$HERE/evidence"
mkdir -p "$EVID"
export DISPLAY="${DISPLAY:-:0}"

measure() { # name rows perfmode [backend]
    local name="$1" rows="$2" perf="$3" backend="${4:-}" log="$EVID/$1.log"
    if [ -n "$backend" ]; then
        ROWS="$rows" SLINT_BACKEND="$backend" SLINT_DEBUG_PERFORMANCE="$perf" /usr/bin/time -v "$BIN" >/dev/null 2>"$log" || true
    else
        ROWS="$rows" SLINT_DEBUG_PERFORMANCE="$perf" /usr/bin/time -v "$BIN" >/dev/null 2>"$log" || true
    fi
    mapfile -t fps < <(grep -oE 'average frames per second: [0-9]+' "$log" | grep -oE '[0-9]+$')
    local rss min="" max="" v deepest
    rss="$(grep -i 'Maximum resident set size' "$log" | grep -oE '[0-9]+')"
    for v in "${fps[@]:1}"; do # drop sample 0 (warm-up)
        [ -z "$min" ] && min=$v && max=$v
        ((v < min)) && min=$v
        ((v > max)) && max=$v
    done
    deepest="$(grep -oE 'deepestRow=[0-9]+' "$log" | grep -oE '[0-9]+' | sort -n | tail -1)"
    echo "$name (rows=$rows $perf ${backend:-gpu}): fps ${min:-?}-${max:-?} (n=${#fps[@]}) deepestRow=${deepest:-?} RSS=${rss:-?}KB"
    echo "rows=$rows perf=$perf backend=${backend:-gpu} fps_min=${min:-NA} fps_max=${max:-NA} deepest_row=${deepest:-NA} rss_kb=${rss:-NA}" >"$EVID/$name.summary"
}

measure gpu-1k 1000 refresh_lazy,console
measure gpu-50k 50000 refresh_lazy,console
measure sw-50k-uncapped 50000 refresh_full_speed,console winit-software

echo
echo "=== SUMMARY ==="
for n in gpu-1k gpu-50k sw-50k-uncapped; do echo "  $(cat "$EVID/$n.summary")"; done
