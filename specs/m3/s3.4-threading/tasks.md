# S3.4 — Conversation threading · Tasks

Derived from `spec.md` + `plan.md` (P8). Engine + store + UI slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] engine `thread::group` (+ `ThreadItem`) — unit + property tests
- [x] engine `fetch_to_new_message`: extract `in_reply_to`
- [x] store: migration #5 (`in_reply_to`); `NewMessage`/`MessageHeader` fields; `upsert_message` +
      `messages_in_folder` updated + tests
- [x] app: `MessageItem.thread_count`; `load_messages` groups + sets count; count badge in the list
- [x] live test (synced reply's `in_reply_to` stored); manual touch

## Verify (acceptance criteria)
- [x] AC1 build/test/clippy/fmt/`cargo deny check` green
- [x] AC2 `thread::group` correct (unit + property)
- [x] AC3 store `in_reply_to` round-trip + header fields
- [x] AC4 LIVE: synced reply `in_reply_to` stored
- [x] AC5 UI count badge; list stays flat (no message hidden)
- [x] AC6 mutants — thread + store covered; imap.rs/main.rs excluded; 0 missed

## Ship
- [x] Code review (guidelines §11)
- [x] Update this tasks file to all-done
- [x] PR merged (one-slice-one-PR, §12)