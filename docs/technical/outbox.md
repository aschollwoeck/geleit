# The outbox — sending offline (SEND-10)

Spec: `specs/outbox/spec.md`. How a send survives having no connection.

## Queue, don't lose

`run_send` builds the RFC 5322 bytes and tries to deliver. The result is classified, not collapsed:

- **Delivered** → file a copy in Sent, expunge any server-side draft copy, drop the local draft. Returns
  `SendStatus::Sent`.
- **Retryable failure** (couldn't connect, TLS, transient 4xx) → the built bytes + envelope
  (`mail_from`, recipients, subject) go into the `outbox` table (migration 19). Returns
  `SendStatus::Queued`; the IPC hands the UI a `queued: bool` so it can say so.
- **Permanent failure** (`lettre::transport::smtp::Error::is_permanent` — a 5xx) → returned as an error,
  **not** queued. Queuing a rejected message would retry it forever; the user must fix or drop it.

The classification lives in `smtp::send`, which now returns `SendError { message, permanent }` instead of
a bare string — that one bit is what decides queue-vs-surface. Pinned by the sink tests: an unreachable
server is `permanent: false` (queued), a `550` recipient rejection is `permanent: true` (surfaced).

## Drain

`run_flush_outbox(account)` sends each pending row (oldest first): delivered → `delete_outbox`; rejected
on retry → `mark_outbox_failed` (stops retrying, stays visible); still-unreachable → left for next time.
It's called by the **scheduler every sweep** and by **Refresh**, both after the flag flush and before
the folder sync — so mail composed offline goes out the moment the connection returns. Delivery reuses
the same `deliver` choke-point as the compose path (SMTP send + best-effort Sent-save), so the two can't
drift.

**Single-flight per account** (`ipc::flush_outbox`, `AppState::draining_outbox`): a sweep and a Refresh
can both call the drain, and SMTP send is *not* idempotent — sending the same row from two concurrent
drains means the recipient gets two copies. So the second drain skips; the first sends everything. This
mirrors the flag-flush single-flight, and it's the reason a flag pull can skip a lock while a send can't.

**A queued draft's provider copy is cleaned up on delivery.** If the message was a resumed copy of a
draft on the provider ('sync drafts'), the outbox row carries `(draft_folder, draft_msgid)`, and the
drain expunges that copy when the message actually goes out — otherwise a sent draft would linger in
the provider's Drafts and reappear locally as a draft the user might re-send.

`failed` rows are kept, not deleted: a rejection the user needs to know about must not vanish. They stop
being retried (`pending_outbox` filters `failed = 0`) and are counted for the indicator.

## The indicator

`Store::outbox_counts() -> (queued, failed)` feeds a quiet line under Compose, shown only when non-zero:
*"N messages waiting to send"*, or *"N couldn't send"* as a warning. The UI refreshes it on load, after a
send, and on the `mail-arrived` event (a sweep may have drained it).

## At-least-once

SMTP can't dedup, and a drop after `DATA` but before `250` is ambiguous. The outbox retries rather than
risk a lost message, removing the row only after a `250` — so the sole duplicate window is a crash
between "accepted" and "row deleted" (a local write, milliseconds). Accepted, not hidden.

## Testability note

The compose→deliver→Sent round trip needs a TLS SMTP server; there is none in CI (the sink is
plaintext, and the local Dovecot is IMAP-only). So the **queue** path (dead port → `Queued` + an outbox
row, and a failed drain leaving it queued) is tested end-to-end through `run_send`/`run_flush_outbox`,
the permanent/retryable split is tested at `smtp::send` against a rejecting sink, and the store CRUD +
counts are unit-tested. The successful-delivery-then-delete composition is covered piecewise (the sink
proves `smtp::send` delivers; the store proves `delete_outbox`), the same limitation the original send
path has always had.


## Acting on the outbox

A failed message that could only be *counted* would be a dead end — stuck forever, its content
invisible. So the outbox is a view: `Store::list_outbox` (all rows, newest first, display fields +
status) behind `ipc::list_outbox`, opened by clicking the indicator (an `outbox_open` pane mirroring the
Drafts pane). Each row offers:

- **Retry** (`retry_outbox`) — `Store::retry_outbox` clears `failed`/`last_error` so the row re-enters
  the queue, then flushes the account immediately so it goes out now if we're online (rather than
  waiting for the next sweep).
- **Discard** (`discard_outbox` → `delete_outbox`) — throw it away, whether waiting or failed.
- **Edit** (failed rows only, `edit_outbox`) — reopen the rejected message in Compose to fix and resend
  it, rather than discard + retype. See below.

The pane is app-wide (the outbox spans accounts), and the actions refresh both the list and the
indicator. Retry re-queuing a message that will just be rejected again is fine — it fails once more and
returns to the failed state; the user has the information (the error) to decide whether to fix the
address (Edit, or discard + recompose) instead.

### Editing a rejected send

`edit_outbox(id)` reconstructs the compose form from the row's stored `raw` bytes — which we built
ourselves at enqueue time, so parsing is a faithful inverse: `message::parse_outbox_for_edit` pulls
To/Cc straight from the headers, the body from the same `text/plain` part the reading pane reads, and
each attachment (name + bytes) via the same `mime::extract_attachment` the viewer uses, materialised to
temp files exactly like resuming a draft. Threading headers are dropped — an edited-and-resent message
starts a fresh send, it isn't a reply to itself.

The row is **left in the outbox** while it's edited; it's discarded (frontend → `discard_outbox`) only
once the edited message is actually sent, so the edited version replaces the original rather than
doubling it. Cancelling the compose therefore loses nothing — the original stays, to retry or discard.
Edit is offered on **failed** rows only, which the scheduler never retries (`pending_outbox` filters
`failed = 0`), so there's no race where the original goes out while it's being edited.
