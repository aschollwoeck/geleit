# S9.4 — Plan

## Engine
Move `run_refresh`/`run_backfill`/`run_remove_account` (UI-agnostic) from the Slint `refresh.rs` into
`geleit_engine::sync_actions`; re-export (S9.3 pattern). The IMAP logic (`sync_folders`/
`sync_folder_incremental`/`backfill_folder`) is untouched. Add a live `#[ignore]` test that drives
the real refresh+backfill against Dovecot.

## Shell
`refresh(account_id, folder)`: await phase-1 recent sync, then spawn a detached thread for the
backfill, forwarding its `on_batch` count as a `sync-progress` Tauri event (`i64`; `-1` = finished).

## Frontend
Refresh button + progress strip; subscribe to `sync-progress` via a small npm-free shim in `early.js`
(`geleitOnSyncProgress`) exposed to Rust as `api::on_sync_progress`. Re-list on recent-sync completion
and on backfill-done.

## Tests
Live refresh+backfill against Dovecot (engine, `#[ignore]`, `dangerous-tls`). The frontend wiring is
verified by that test + rendering (no click injection available).
