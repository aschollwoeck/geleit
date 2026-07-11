# S9.3 — Message list: virtualization, threading, flags, actions

**Milestone:** M9. **Constitution:** P1 (local-first), P3 (calm and fast; a perf regression is a
defect), P6 (integrity — never lose/dupe mail).

## What it delivers

The message list becomes the real thing: it scales to a big mailbox, shows conversations, and lets
you act on mail (star, archive, delete, move, spam, mark unread) — with the same optimistic-local +
worker-write-back model the Slint app proved (M5), reusing the engine and store as-is.

| | Story | Acceptance |
|---|---|---|
| **S9.3-1** | A 50k-message folder is still instant. | The list is **virtualized** — only visible rows are in the DOM. Scrolling stays smooth; opening the folder is instant. |
| **S9.3-2** | I see conversations. | Messages in the same thread show a **conversation · N** marker (engine `thread::group`, reused). |
| **S9.3-3** | I can star / unstar. | Toggling the star updates the row instantly (optimistic) and writes back to the server on a worker; a failure self-heals on refresh. |
| **S9.3-4** | I can archive / delete / move / mark spam. | Each acts optimistically (row leaves the list) and writes back via the existing engine paths; **no mail is lost or duplicated** (M5 model). |
| **S9.3-5** | I can mark a message unread again. | Brings the dot back, persisted, and written back to the server. |
| **S9.3-6** | It stays calm and fast. | Actions never block the UI (P1); the list never janks. |

## How

- **Virtualization:** a windowed list — render only the rows in (and just around) the viewport,
  spacer above/below for the scrollbar. No new dependency; a small amount of scroll math in
  `geleit-ui`. This is the one genuinely *new* UI mechanism in the slice.
- **Actions:** new IPC commands (`set_flagged`, `archive`, `delete`, `move_to`, `set_spam`,
  `set_seen`) that each do the optimistic store write **and** kick the engine's existing worker
  write-back (`refresh::run_set_flag` / `run_move` etc. — already account-scoped, already proven).
  The frontend removes/updates the row immediately.
- **Threading:** `thread::group` over the loaded page → a per-row conversation count. Pure, reused.

## Reuse, not reinvention

The engine and store already do the hard parts: `set_flagged`/`set_seen`/`delete_message`,
`imap::set_flag`/`move_message`, `thread::group`, and the optimistic-then-write-back discipline. S9.3
is the **UI + IPC** for them in the new shell — it must not fork that logic.

## Out of scope

Refresh/sync (S9.4), multi-select bulk actions (a follow-up if time allows — flag it, don't smuggle
it), compose (S9.5), search/settings (S9.6). Server flag *read-back* stays as it is.
