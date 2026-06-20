# S0.3 — Slint virtualized message-list spike · Plan (the HOW)

Implements `spec.md`. References ADR-0001. Throwaway spike — produces evidence, not shippable code.

## Location & isolation
- `spikes/s0.3-list-render/` — standalone crate **excluded from the workspace**
  (`exclude` in root `Cargo.toml`). Not touched by `cargo build --workspace` or CI.

## Library
- **`slint` 1.16**, using the inline `slint!` macro (no build.rs needed for a spike).
- Default backend (winit + femtovg/GL). Fallback `SLINT_BACKEND=winit-software` if GL is
  unavailable (libGL is present locally, so GPU path is expected).

## Harness (`src/main.rs`)
- A `Window` containing a std-widgets `ListView` bound to a model of `ROWS` rows
  (`ROWS` env var, default 50000).
- Row delegate (height 64px): unread dot, sender (bold if unread), subject, grey snippet, date,
  attachment marker — realistic message-row content.
- Model built in Rust as a `VecModel<Row>` of generated `Row` structs.
- **Programmatic scrolling:** root `scroll-y` property bound to the ListView `viewport-y`; a
  repeating `slint::Timer` (~16ms) advances `scroll-y` and wraps around, so rendering reflects
  continuous scrolling through the whole list.
- **Auto-exit:** a single-shot `slint::Timer` calls `slint::quit_event_loop()` after ~6s.

## Measurement (`run-spike.sh` → `evidence/`)
- **FPS:** run with `SLINT_DEBUG_PERFORMANCE=refresh_full_speed,console` — Slint renders
  uncapped and prints the achieved frame rate to stderr; capture it. Uncapped FPS ≥ 60 (with
  headroom) proves smooth 60fps scrolling is achievable.
- **Memory:** run under `/usr/bin/time -v`; record **Maximum resident set size**.
- **Contrast:** run at `ROWS=1000` and `ROWS=50000`. Virtualization is shown if FPS stays high
  and RSS does not grow like per-row widgets would (the delta should be ~the row *data*, not
  50k instantiated delegates).
- Capture FPS + max-RSS for both runs into `evidence/`.

## Verification (maps to acceptance criteria)
- AC1 renders 50k rows + scrolls: harness runs without error at `ROWS=50000`.
- AC2 ≥60fps: captured FPS at 50k ≥ 60 (full-speed).
- AC3 bounded memory: RSS(50k) modest and not ~50× RSS(1k); both recorded.
- AC4 findings → ADR-0001 / S0.4.

## Findings
`docs/technical/s0.3-list-spike-findings.md` — FPS + RSS numbers (1k vs 50k), the ADR-0001
recommendation (confirm Slint, or revisit), and any caveats (e.g. software-renderer fallback).

## Risk / fallback
If 50k rows can't sustain ≥60fps even with the GPU backend, record it as a fail feeding S0.4
(options: tune the delegate, software vs GPU, or reconsider the list strategy).
