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
- [x] Code review agent — 3 findings, all fixed (below)
- [x] In-app: the list renders virtualized, the action bar shows on an open message

## Code review — findings acted on
The review confirmed **no server-side mail loss** (`uid_mv` is atomic; `message_location` requires a
synced uid; the write-back move out of `refresh.rs` is byte-for-byte clean; threading indices line up).
Three fixes:

| # | Finding | Fix |
|---|---|---|
| 1 | **Integrity (high):** `move_to_role` deleted the local row *before* the server move, but the "self-heals on refresh" safety net doesn't exist until S9.4 — so a failed move left the message absent locally with no way back. | Restructured: the local delete now happens **only after the server move succeeds**. A failed move never touches the store, so nothing is lost and no refresh is needed. |
| 2 | **Performance (high):** the scroll-path closures cloned the *entire* `Vec<Message>` on every scroll tick (twice) — defeating virtualization. | `messages.with(Vec::len)` for the length; clone only the visible window (`with(\|all\| all[first..first+count].to_vec())`). |
| 3 | **Robustness (low):** `visible_range`'s `total - first` could underflow if a stale scroll offset outran a shrunken list (a trap for the bulk-actions follow-up). | Clamp `first` to `total-1`; added a test for the shrunken-list case. |

Also: viewport height is now measured on mount (a taller-than-estimated viewport rendered fully from
the start). The `snapshot` clone on the move *click* path was left (click path, not scroll path).

## Honest verification note
The build environment can't inject clicks, so the **action buttons** were verified by (a) the pure
resolvers being unit+mutation-tested, (b) the write-backs being the *same* engine code the Slint app
shipped, now single-sourced, and (c) the action bar rendering in-app. A click-driven archive/delete
was not injection-tested end-to-end here.

## Not in scope
Refresh/sync (S9.4) · multi-select bulk actions (a deliberate follow-up — not smuggled in) ·
compose (S9.5) · search/settings (S9.6).
