# S9.8 — Tasks

Status: **complete** — the P4 ceilings are a CI gate; the app passes with room to spare.

- [x] `GELEIT_PERF=1` → the window's `on_page_load` prints `GELEIT_READY` (first-paint marker, WM-independent)
- [x] `scripts/perf-budget.sh`: binary size (always) + cold start + idle RSS (with a display), vs the
      P4 ceilings; exits non-zero on breach; skips display-dependent checks cleanly without `$DISPLAY`
- [x] CI `perf-budget` job (PRs): installs xvfb + webkit/gtk, builds release, runs the harness under `xvfb-run`
- [x] **Proven the gate can fail** (lowering a ceiling below the measured value fails it)
- [x] constitution P4 table annotated with how each budget is enforced (message-open is architecturally
      bounded — local read + local render, no network — not separately timed)

## Measured (release, this machine)
binary **18 MB** ≤ 30 · cold start **918 ms** ≤ 1200 · idle RSS **147 MB** ≤ 280 — all ✓.

## Gates
- [x] fmt · clippy `-D warnings` · tests · deny · the perf harness itself runs green
- [ ] Code review agent → then merge
