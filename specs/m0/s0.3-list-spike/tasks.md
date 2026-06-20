# S0.3 — Slint virtualized message-list spike · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md`. Kept current (P8). Throwaway spike.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build (throwaway harness)
- [x] Exclude `spikes/s0.3-list-render` from the workspace (root `Cargo.toml`)
- [x] Spike crate `Cargo.toml` (slint 1.16)
- [x] `src/main.rs` — ListView of ROWS message-like rows, programmatic scroll, auto-exit
- [x] `run-spike.sh` — capture FPS + max RSS for 1k/50k (GPU) and 50k (software)

## Verify (acceptance criteria — measurable; RELEASE build, full-list traversal)
- [x] AC1 renders 50,000 rows + scrolls deep (deepest rendered row logged: ~40k GPU / ~50k sw)
- [x] AC2 ≥60fps at 50k on GPU: 64–65fps real scroll (below the 74.98Hz vsync ceiling, so GPU
      cost is measured and still ≥60)
- [x] AC3 bounded memory: 1k→50k adds only ~16 MB (~335 B/row = data); both captured
- [x] AC4 ADR-0001 outcome recorded (S0.3 PASS; both gates now passed)
- [x] (extra) virtualization proven 2 ways (deep rows rendered at ≥60fps + data-only memory delta)

## Document
- [x] `docs/technical/s0.3-list-spike-findings.md` — release FPS/RSS + traversal + recommendation
- [x] ADR-0001 status updated (both gates PASSED)
- [x] (No end-user manual — spike slice)

## Ship
- [x] Code review of the slice diff (guidelines §11) — addressed: reviewer caught a debug build
      + 0.5%-coverage scroll (couldn't prove deep virtualization or motion). Redone with a
      release build, full-list traversal (logged), and lazy-mode real-redraw FPS; corrected the
      software-fallback number (≈57fps release, not ≈19fps debug) and reworded all claims
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)
