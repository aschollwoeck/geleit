# S2.6 — Non-blocking sync status · Plan (the HOW)

Implements `spec.md`. UI-only (Slint + the refresh worker wiring).

## Slint (`main.rs`)
- `in property <string> sync-status;` (calm progress; non-empty = show).
- A calm strip in the message-list column (after the header divider, before the error banner /
  ListView): `surface` bg, a small `accent` dot + `muted` text (`sync-status`), a bottom divider.
  Shown via `if root.sync-status != ""`. Distinct from the existing danger `status` banner.

## Refresh worker (`on_refresh`)
- On start (after `set_refreshing(true)`): `set_sync_status("Checking for new mail…")`.
- After phase-1 incremental: `post_reload` (clears the error `status`) — leave `sync-status` set.
- Backfill `on_batch(n)`: post `set_sync_status(format!("Catching up… {n}"))` (was the danger
  `status`). 
- On completion: post `set_refreshing(false)`, `set_sync_status("")`, reload.
- On phase-1 error: `set_refreshing(false)`, `set_sync_status("")`, `set_status(err)` (danger).
- `on_remove_account` / setup errors keep using the danger `status` only.

## Verify
build/test/clippy/fmt/`cargo deny check`; launch against a seeded/real store and observe the calm
status during refresh + backfill; errors still show in the danger banner. `cargo mutants` unchanged.
