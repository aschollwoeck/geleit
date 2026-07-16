# Outbox — sending survives being offline (SEND-10)

**Constitution:** P1 (the UI never waits), P3 (calm), P8 (spec-driven).
**Story:** *I wrote a reply on the train, hit Send, and closed the lid. It should go out when I'm back
online — not be lost because there was no signal.*

## The gap

`run_send` built the message, tried SMTP once, and on failure returned an error. Offline, that error
*was* the outcome: the mail didn't go, and nothing remembered it should. The composed message existed
only in the closed compose window.

## The rule

> A send that can't reach the server is **queued**, not lost — and retried until it goes out.

- **Queue on a retryable failure.** Couldn't connect, TLS failed, a transient 4xx — the ordinary
  offline case. The built RFC 5322 bytes + the envelope go into an `outbox` table (migration 19), the
  compose window closes, and the toast says *"Queued — will send when you're back online."*
- **Surface a permanent rejection.** A 5xx (`lettre`'s `is_permanent`) means the server *answered* and
  refused — a bad address, a policy block. Retrying never helps, so it is **not** queued: the error is
  shown and the compose window stays, so the user can fix or drop it.
- **Drain on every sweep and on Refresh.** The scheduler already runs; when it reaches the server it
  sends everything waiting, files each in Sent, and removes it from the outbox. A message that is
  rejected *on retry* (rare) is marked failed — it stops retrying and is surfaced, never looped or
  silently dropped.

## What the user sees

A quiet line under **Compose**, only when something is waiting: *"2 messages waiting to send"*, or
*"1 couldn't send"* as a warning when the server rejected one. A quiet outbox is invisible.

Clicking it opens the **Outbox** in the middle pane: each message shows its recipient, subject and
status. A failed one shows *why* it was rejected and offers **Retry** (re-queue and try again now) and
**Discard**; a waiting one offers **Discard** (cancel it before it goes). So a message that couldn't be
sent is never a dead end.

## Concurrency and clean-up

Only one drain of an account runs at a time (a scheduler sweep and a Refresh can't both send the same
message — that would be a duplicate the recipient sees). If a queued message was a resumed copy of a
draft on the provider ('sync drafts'), the outbox carries that reference and expunges the copy once the
message goes out — so a queued-then-sent draft doesn't linger in the provider's Drafts folder.

## At-least-once, deliberately

SMTP has no natural dedup, and a connection that drops after `DATA` but before the `250` is
ambiguous — the server may have the message or not. Faced with retry-and-maybe-duplicate versus
skip-and-maybe-lose, the outbox **retries**: a rare duplicate is better than a lost message. The row is
removed only after a `250`, so the only duplicate window is a crash between "server accepted" and "row
deleted" — milliseconds, a local write.

## Out of scope (named)

**Editing** a failed message to fix a bad address (retry and discard exist; editing the content does
not — the workaround is discard + recompose). Queuing moves/deletes — those are server-first, so a failed one doesn't diverge. A true
scheduled-send / "send later". Per-message delivery receipts. A few genuinely-permanent *client-side*
failures (no compatible auth mechanism, STARTTLS unsupported, a rejected TLS certificate) are classed
retryable by the SMTP library, so they queue and retry rather than surfacing — a misconfiguration that
shows up at account setup, not mid-use; narrowing that is a follow-up.
