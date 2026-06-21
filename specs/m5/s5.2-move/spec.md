# S5.2 — Archive / Delete-to-Trash / Move + write-back (ORG-1/2/3, SYNC-5) · Spec

Slice of **M5**. Archive, delete (to Trash), and move a message to a folder — optimistic locally,
written back to the server with no dupes/loss.

## Model
Each action = **optimistic local remove** of the message + a worker **IMAP `MOVE`** (source→target).
The message reappears in the target folder on the next sync (with its correct new UID); if the move
fails, it reappears in the source on the next refresh (no loss). Avoids the UID-changes-on-move
duplicate problem by not keeping the moved row locally.

## In scope
- Engine: `imap::move_message(config, secrets, source, uid, target)` (`UID MOVE`).
- Store: `delete_message(id)` (local optimistic remove; body/attachments cascade).
- App: viewmodel `find_folder` (locate Archive/Trash by name); reading-pane **Archive / Delete /
  Move…** actions; a "Move to…" folder picker; `perform_move` (optimistic remove + clear reading pane
  + worker `run_move`, calm "it'll return on refresh" on failure).

## Out of scope
- COPY+EXPUNGE fallback when the server lacks MOVE; creating Archive/Trash if absent (status instead);
  permanent delete / empty-trash (S5.3); bulk (S5.6).

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. `find_folder` exact-then-contains + `delete_message` remove — tested.
3. App: Archive/Delete/Move remove the message optimistically + write back; missing Archive/Trash →
   calm note; failure → message returns on refresh (maintainer eyeballs).
4. `cargo mutants` — store + viewmodel 0-missed (engine `move_message` is live-tested glue).

## Deliverables
- `move_message` + `run_move`; `delete_message`; `find_folder`; Archive/Delete/Move UI + picker +
  `perform_move`.
