# S9.3 — Plan

## Engine: single-source the write-backs (`geleit_engine::sync_actions`)

The optimistic-write-back helpers (`run_set_flag`, `run_set_seen`, `run_move`,
`run_delete_permanently`, plus `account_imap`/`runtime`/`to_config`) live in `geleit-app`'s
`refresh.rs`. Both UIs need them, so move them to `geleit_engine::sync_actions`; `refresh.rs`
re-exports so the Slint app is unchanged (same pattern as S9.1's `open_store`). The actual IMAP logic
(`imap::set_flag`/`move_message`/…) is already in the engine and is **not** touched — only the thin
"open store → read config → block_on" wrappers move.

## Shell: action commands (optimistic + worker write-back)

New IPC commands, each following the M5 model — **optimistic local write, then a worker thread does
the server write-back; a failure self-heals on the next refresh:**

```
set_star(id, on)      store.set_flagged   + run_set_flag  (needs uid + folder)
set_unread(id)        store.set_seen(false) + run_set_seen
archive(id)           move → Archive
trash(id)             move → Trash
set_spam(id, on)      move → Spam / back to Inbox
```

Move actions need the message's `(folder, uid)` (`store.message_location`) and the account's
folder names (`store.folders_for_account` → match Archive/Trash/Spam by `folder_rank`, reusing the
same classification the rail uses). Optimism: the row is removed from the list immediately; the
worker runs; on failure a toast appears and the next refresh restores truth.

`spawn_blocking` for the DB write; a detached `std::thread` for the network write-back (it must not
hold the UI up, and it can outlive the command).

## Frontend: virtualization + threading + actions

- **Virtualization:** a windowed list. Track scroll position + row height; render only the rows in a
  viewport-plus-margin window; pad with a top/bottom spacer so the scrollbar is correct. No new dep —
  ~40 lines of scroll math. This is the one new mechanism.
- **Threading:** `thread::group` over the loaded page → a per-thread count; rows with `N > 1` show
  `conversation · N`. Pure, reused from the engine.
- **Row actions:** hover actions / a selected-row action bar (star, archive, trash, spam, mark
  unread), each calling the matching command and updating the list optimistically.

## Tests

- **Engine:** `sync_actions` move-target resolution is pure and testable (given folder names → the
  right target); the network wrappers stay integration-only (like `imap.rs`, excluded from mutants).
- **Shell:** the pure parts of the action commands (target-folder choice, `message_location` use).
- **Frontend:** the virtualization window math (`visible_range(scroll, height, total)`), pure and
  unit-tested; thread-count mapping.
- **In-app:** screenshot a big seeded folder (smooth scroll), a conversation marker, and a star
  toggle.

## Risks

| Risk | Handling |
|---|---|
| Moving helpers destabilizes the shipping Slint app | Re-export, don't rewrite call sites; rebuild + test `geleit-app` before commit |
| Virtualization math off-by-one → blank rows / jitter | Pure `visible_range`, unit-tested at boundaries |
| Optimistic remove loses mail on write-back failure | M5 model — local remove is provisional; refresh restores; never expunge on the optimistic path |
