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
- [x] Code review agent + a real CI run — 5 findings, all fixed (below)

## Code review + CI — findings acted on
The first CI run **failed** (cold start 18589 ms) — the webview never painted under xvfb, and the
review found a HIGH false-pass in the RSS check. Both fixed:

| # | Finding | Fix |
|---|---|---|
| CI | **Webview didn't render headless** — WebKitGTK's default DMABUF/GPU path hangs under xvfb, so first paint never happened → false *fail*. | Export `WEBKIT_DISABLE_DMABUF_RENDERER=1` (+ compositing/software-GL) in the harness — the documented headless fix. Cold start now 849 ms locally. |
| 1 | **False pass (high):** RSS read only the parent (WebKit is multi-process — the web-content process, the likely regression site, was invisible), and a dead pid → `0 MB` → ✓. | Sum across the whole process tree, and use **PSS** not RSS (summing RSS triple-counts WebKit's shared libs: 349 MB vs the true 135 MB PSS). A vanished process is a hard **failure**, never 0. |
| 2 | **Binary ceiling ~1 MiB loose** (integer-MB truncation). | Compare raw bytes against `30*1048576`. |
| 3 | **RSS was a single 3rd-run sample.** | Median of 3, like cold start. |
| 4 | **Median masked a 1-in-3 boot hang.** | Any run that never paints (or whose process vanishes) is a hard failure, regardless of the median. |
| 5 | **Constitution overclaim** ("CI fails on any ceiling"; "no network → cannot exceed 100 ms" non-sequitur). | Reworded: CI hard-gates the 3 *timed* budgets; message-open is **not CI-timed**, bounded by design (local read + local render, no network/I/O). |

Measured after the fixes (real display): binary **18.5 MB**, cold start **849 ms**, idle RSS(PSS) **135 MB**.
