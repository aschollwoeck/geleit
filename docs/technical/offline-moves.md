# Offline moves — filing mail with no connection (OFF-4)

Spec: `specs/offline-moves/spec.md`. How Archive / Delete / Spam / Move to… survive having no
connection — the sibling of the [outbox](outbox.md) (which does the same for *sending*) and of the
`flags_dirty` write-back (which does it for *read/star*).

## One column, two jobs

A move is recorded as a **`message.pending_move`** marker (migration 22): `NULL` = a normal message; a
folder **name** = "this message is queued to move there." That single column does two jobs, exactly like
`flags_dirty`:

1. **It hides the message** from every listing (`messages_in_folder`, `messages_in_all_inboxes`,
   `total_inbox_unread`, `folder_unread_counts`) the instant it's set — so the move looks instantaneous
   and needs no connection.
2. **It is the durable queue.** The message row still holds its source folder and IMAP uid, which is
   everything the server move needs, so no separate table is required.

The row is **not** deleted and **not** moved on the server when the marker is set. The message simply
disappears from view and waits.

## Mark, then flush

`move_to_role` / `move_to_folder` (`geleit-app/src/ipc.rs`) plan the move (resolve the target folder,
reject an unknown one or a no-op), call `store.queue_move(id, target)`, then call `flush_moves` to push
it. Online, the flush lands the move before the command returns and the list settles at once; offline,
the flush is a quick no-op that leaves the move queued.

## Drain

`run_flush_moves(account)` (`geleit-engine/src/sync_actions.rs`) drains the queue. Moves are grouped by
**source folder** so each is one session (`imap::move_batch`, modelled on `push_flags`: connect and
`SELECT` the source once, then `UID MOVE` each message to its target). That grouping is also what lets
the drain tell the two failure kinds apart, and every outcome has exactly one of three fates:

- **Landed** → `delete_message` the local row. Its source folder no longer has it on the server, and a
  sync of the target folder re-adds it with the server's **new** uid — so deleting locally can't lose or
  duplicate it.
- **Unreachable** (couldn't connect or select the source — the offline case) → the whole group is left
  queued, marker untouched, and the next sweep retries. The mail is **never expunged**; it stays safe in
  its source folder the whole time it waits.
- **Refused** (the session is up but the server rejected the `UID MOVE` — an unknown target, or a uid
  that's already gone) → `clear_pending_move` drops the marker so the message **reappears in its source
  folder** rather than hiding forever behind a move that can never complete. Retrying such a move never
  helps, so the drain stops hiding it and the user sees it back where it was.

It's called by the **scheduler every sweep** (after the flag flush, before the outbox drain) and by each
move command, so a move made offline reaches the server the moment the connection returns.

**Single-flight per account** (`ipc::flush_moves`, `AppState::draining_moves`): a sweep and a fresh move
can both call the drain, and a second `UID MOVE` for a message already gone from its source folder is an
*error*, not a no-op — so the second caller skips and the first drains everything. This mirrors the
outbox's `draining_outbox`.

## Why re-sync doesn't undo it

The property that makes the marker safe: `pending_move` is **absent from `upsert_message`'s
`ON CONFLICT` update set**. While a move waits, the message is still in its source folder on the server,
so a re-sync of that folder re-touches the row — but the marker survives, so the message stays hidden and
stays queued rather than popping back into the inbox. (Same trick as `notified` and `filtered`.)

## Scope and limits

- **Covers** Archive, Delete-to-Trash, Spam, and Move to… — every action that is an IMAP *move*.
- **Does not cover** permanent delete (`empty_trash`, `delete_forever`). Those are irreversible expunges
  you do while looking at the Trash, online; they stay server-first.
- **A message is never hidden indefinitely.** Offline, a move waits (queued) until the connection
  returns. If the server is reachable but *refuses* the move, the message is un-hidden and comes back to
  its source folder — so there is no state where mail is both invisible locally and impossible to move.
