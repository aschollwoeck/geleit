# Offline moves — organizing survives being offline (OFF-4)

**Constitution:** P1 (the UI never waits), P3 (calm), P8 (spec-driven).
**Story:** *On the train I archived a dozen threads and moved a few into folders. It should still be
tidied when I'm back online — not snap back because there was no signal.*

## The gap

`move_to_role` / `move_to_folder` did the server move **first** and only then removed the local row.
Offline, the server move failed, the command returned an error, and nothing was recorded — the archive
or move simply didn't happen. Filing mail needed a connection, even though reading, searching, composing
(OFF-1/2/3) already didn't.

## The rule

> A move that can't reach the server is **recorded and hidden**, not lost — and pushed when the
> connection returns.

- **Mark, don't move.** The move is written as a `message.pending_move` marker (migration 22) holding
  the target folder name. The message vanishes from every listing at once (the move feels instant, P1),
  the row stays put, and nothing touches the server yet.
- **The marker is the queue.** The message row already carries its source folder and uid — all the
  server move needs — so `pending_move` doubles as a durable per-message queue, no side table.
- **Drain on every sweep.** `run_flush_moves` pushes each queued move, grouped by source folder into one
  session. Three outcomes: a move that **lands** has its local row deleted (it reappears in the target
  folder with the server's new uid on the next sync — never duplicated); one that can't reach the server
  (**offline**) stays queued and is retried; one the server is up for but **refuses** (unknown target, a
  uid already gone) is un-hidden so it returns to its source folder rather than hiding behind an
  impossible move. The mail is **never expunged**, so nothing is lost, and nothing is hidden forever.
- **Re-sync must not undo it.** `pending_move` is left out of `upsert_message`'s `ON CONFLICT` set, so a
  re-sync of the source folder — where the message still is until the move lands — preserves the marker
  rather than re-showing the message.
- **Single-flight per account.** A sweep and a fresh move mustn't drain the same rows at once (a second
  `move_message` for an already-moved message is an error), so the drain is single-flight like the
  outbox's.

## Scope

- **In:** Archive, Delete-to-Trash, Spam, Move to… — every action that is an IMAP *move*.
- **Out:** permanent delete (`empty_trash`, `delete_forever`) — irreversible expunges done while online,
  left server-first.

## What the user sees

Nothing new: the message leaves its folder the instant they act, online or off, exactly as before. The
only change is that offline it now *stays* filed and reconciles on reconnect, instead of snapping back
with an error.

## Verified

- Unit (`geleit-store`): a queued move hides the message from every listing — folder list, All-Inboxes,
  unread counts/badge, **search**, and notifications — and appears in `pending_moves` with the right
  source/uid/target; a re-sync of the source folder preserves the marker; clearing the marker brings the
  message back; a local-only (no-uid) message is never queued.
- Live (`geleit-app/examples/live_offline_move.rs`, against local Dovecot): a move to a **bogus** folder
  is refused and the message is un-hidden back into INBOX (never lost, never stuck); re-aimed at a
  **real** folder it reaches the server — gone from INBOX, present in the target.
