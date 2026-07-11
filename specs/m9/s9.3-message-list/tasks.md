# S9.3 — Tasks

Status: **complete** — gates green, verified in-app.

## Engine — single-source the write-backs
- [x] `geleit_engine::sync_actions` — moved `run_set_flag`/`run_set_seen`/`run_move`/
      `run_delete_permanently`/`run_empty_folder` + `account_imap`/`runtime`/`to_config` out of the
      Slint `refresh.rs`; `refresh.rs` re-exports/imports them (Slint app unchanged, still builds + tests)
- [x] The actual IMAP logic (`imap::*`) was **not** touched — only the thin wrappers moved

## Store
- [x] `account_for_message(id)` — routes a per-account write-back

## Shell — action commands (optimistic + worker write-back, M5 model)
- [x] `set_star` · `set_unread` · `move_to_role` (archive/trash/spam/inbox)
- [x] `spawn_writeback` — detached thread; failure self-heals on refresh; **never expunges** on the
      optimistic path, so a failed write-back can't lose mail
- [x] `resolve_folder` / `FolderRole` — role → the account's actual folder name; **declines** if the
      account has no such folder rather than inventing a destination. Pure, unit- + mutation-tested.
- [x] Threading counts computed in the shell (`with_thread_counts`, engine `thread::group`) — the
      frontend can't depend on the engine, so it only sees the finished count. Unit- + mutation-tested.

## Frontend
- [x] **Virtualization** — `visible_range(scroll, viewport, row, total)` (pure, mutation-tested at the
      boundaries) drives a windowed list: a full-height sizer + a translated window holding only the
      visible rows (~23), so only that slice is cloned into the DOM. Fixed 64px rows.
- [x] `conversation · N` marker when `thread_count > 1`
- [x] Reading-pane action bar: Star · Archive · Delete · Spam · Unread — optimistic list update +
      the matching command; a move removes the row and closes the pane, restoring it if the account
      has no such folder.

## Gates
- [x] fmt · clippy `-D warnings` · tests · `cargo deny` · wasm build · boundary check
- [x] mutants on the new pure logic — **143 caught, 0 missed**
- [x] Slint `geleit-app` still builds + its tests pass (the write-back move didn't regress it)
- [ ] Code review agent → then merge
- [x] In-app: the list renders virtualized, the action bar shows on an open message

## Honest verification note
The build environment can't inject clicks, so the **action buttons** were verified by (a) the pure
resolvers being unit+mutation-tested, (b) the write-backs being the *same* engine code the Slint app
shipped, now single-sourced, and (c) the action bar rendering in-app. A click-driven archive/delete
was not injection-tested end-to-end here.

## Not in scope
Refresh/sync (S9.4) · multi-select bulk actions (a deliberate follow-up — not smuggled in) ·
compose (S9.5) · search/settings (S9.6).
