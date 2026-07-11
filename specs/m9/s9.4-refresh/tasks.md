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
- [x] Code review agent — 5 findings, all fixed (below)
- [x] In-app: the Refresh button + list header render; layout intact

## Code review — findings acted on
The function move, re-exports, payload encoding, and `closure.forget()` were found solid. Five fixes:

| # | Finding | Fix |
|---|---|---|
| 1 | **Stale-folder race (high):** `do_refresh`'s post-sync re-list bypassed the `request` epoch — switching folders mid-sync could clobber the new folder with the old folder's mail. | The re-list now bumps + checks the epoch, exactly like `choose_folder`. |
| 2 | **Stale-folder race (high):** the `-1` completion re-list was unguarded too. | Same epoch guard applied. |
| 3 | **Overlapping refreshes (medium):** a second Refresh could start while the first backfill still streamed, interleaving two counts into one strip. | Refresh is blocked (and the button disabled) while `catchup` is `Some` — i.e. until the backfill signals done. |
| 4 | **Completion not guaranteed (low):** a panic in the backfill thread skipped the `-1` emit, sticking the strip; and a backfill `Err` was indistinguishable from success. | A **`Drop` guard** emits the sentinel no matter how the thread leaves; `-1` = clean, `-2` = stopped early → the UI shows a calm "will resume next refresh" note (S9.4-4). |
| 5 | **Doc (low):** `run_remove_account` lost its "run on a worker thread" note in the move. | Restored; blank lines between the moved functions restored. |

## Honest note
The **event-streamed progress strip** was verified by: the underlying refresh+backfill passing live
against Dovecot (the callback fires), standard Tauri event plumbing, and the button/strip rendering
in-app. A live end-to-end sync *driven from a click in the running window* wasn't injection-tested
(no xdotool) — but the callback path it forwards is the one the live test exercises.

## Not in scope
Background auto-sync on a timer (a follow-up — refresh stays manual); compose (S9.5); search/settings
(S9.6).
