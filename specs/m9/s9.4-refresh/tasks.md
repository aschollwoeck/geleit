# S9.4 — Tasks

Status: **complete** — gates green; the refresh+backfill path **live-verified against Dovecot**.

## Engine
- [x] `run_refresh` / `run_backfill` / `run_remove_account` moved from the Slint `refresh.rs` into
      `geleit_engine::sync_actions`; re-exported so the Slint app is unchanged (S9.3 pattern)
- [x] **Live test** (`dangerous-tls`, `#[ignore]`): the exact refresh→backfill path the command drives,
      against local Dovecot — pulls INBOX mail and streams progress via the `on_batch` callback. **Passes.**

## Shell
- [x] `refresh(account_id, folder)` command: phase 1 syncs recent mail (awaited), phase 2 backfills
      older mail on a detached thread, **emitting `sync-progress` events** (`i64`: batch count, `-1` = done)
- [x] Reuses the engine wrappers; no IMAP logic duplicated

## Frontend
- [x] `sync-progress` event subscription (`api::on_sync_progress`) via a small npm-free shim
- [x] Refresh button (list header) → "Refreshing…" while in flight; disabled during
- [x] Progress strip: "Checking for new mail…" then "Catching up… N", distinct from the error toast
- [x] Re-lists the folder when recent sync lands **and** when the background catch-up finishes — so
      new mail appears and any failed-write-back divergence (S9.3) heals

## Gates
- [x] fmt · clippy `-D warnings` · tests · `cargo deny` · wasm · boundary · Slint app still builds
- [x] `sync_actions.rs` added to the mutants exclude (network glue, like `imap.rs`)
- [ ] Code review agent → then merge
- [x] In-app: the Refresh button + list header render; layout intact

## Honest note
The **event-streamed progress strip** was verified by: the underlying refresh+backfill passing live
against Dovecot (the callback fires), standard Tauri event plumbing, and the button/strip rendering
in-app. A live end-to-end sync *driven from a click in the running window* wasn't injection-tested
(no xdotool) — but the callback path it forwards is the one the live test exercises.

## Not in scope
Background auto-sync on a timer (a follow-up — refresh stays manual); compose (S9.5); search/settings
(S9.6).
