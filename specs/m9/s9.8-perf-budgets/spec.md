# S9.8 — Enforce the P4 performance budgets in CI

**Milestone:** M9 (final slice). **Constitution:** P4 (amended in M9 — "leanness is measured, not
asserted"), P3 (calm and fast).

## What it delivers

The teeth for P4's promise: the performance ceilings become a **CI gate**, not a slogan. A PR that
makes the app fatter or slower past a ceiling fails the build.

| | |
|---|---|
| **S9.8-1** | `scripts/perf-budget.sh` measures the release app against the P4 ceilings and exits non-zero on a breach. |
| **S9.8-2** | A CI `perf-budget` job runs it **headless under xvfb** on every PR. |
| **S9.8-3** | The gate is real — proven to fail when a ceiling is lowered below the measured value. |

## Ceilings (constitution P4)

| Budget | Ceiling | How |
|---|---|---|
| Binary size (stripped) | 30 MB | `stat` the release binary |
| Cold start (exec → first paint) | 1200 ms | median of 3; timed to the `GELEIT_READY` marker the app prints on first page load under `GELEIT_PERF=1` (window-manager-independent, works under xvfb) |
| Idle RSS (window open) | 280 MB | `/proc/<pid>/VmRSS` after settle |
| Message-open (click → rendered) | 100 ms | architecturally bounded (local SQLite read + local iframe render, **no network**); not separately timed |

## Measured (this machine, i7-2600 — a pessimistic floor)

Binary **18 MB**, cold start **918 ms**, idle RSS **147 MB** — all comfortably under budget.

## How

- A `GELEIT_PERF=1` gate on the window's `on_page_load` prints `GELEIT_READY` — a first-paint marker
  that needs no window manager, so cold start is measurable under headless xvfb.
- `scripts/perf-budget.sh`: binary size always; cold start + RSS when a display is present (skipped
  with a note otherwise, so it's still useful locally without a GUI).
- CI job installs `xvfb` + webkit/gtk, builds release, runs the harness under `xvfb-run`.

## Out of scope

Nothing — this is the last M9 slice. (Message-open timing could be added later if the open path ever
gains I/O; see the P4 note.)
