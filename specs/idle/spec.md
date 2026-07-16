# Instant new-mail with IMAP IDLE (RFC 2177)

**Constitution:** P1 (the UI never waits), P3 (calm + fast — latency is a defect), P8 (spec-driven).
**Story:** *Mail should land in front of me the moment it arrives — not up to five minutes later.*

## Why

Background sync polls every 5 minutes. That's fine as a floor, but a message can sit unseen for the
whole interval. IMAP IDLE (RFC 2177) lets the server *push*: the client says `IDLE` and the server
speaks up the instant something changes. Most providers support it (Dovecot, Gmail, GMX, Fastmail…).

## The design — a trigger, not a second sync

An IDLE watcher per account holds one connection to the INBOX. When the server pushes, the watcher does
the **smallest possible thing**: it wakes the existing sync scheduler (`AppState::wake_sync`), which
already syncs, notifies, and updates the badge — the exact same path a user-pressed Refresh drives. So
IDLE adds a low-latency trigger on top of everything already built, not a parallel sync/notify path that
could drift from it.

- **One connection, re-IDLEd** just under the 29-minute RFC 2177 limit (28 min), not reconnected each
  cycle — so an idle mailbox holds its connection quietly.
- **The poll stays** as the safety net: for providers without IDLE, for the reconnect gaps, and for
  folders other than the INBOX. IDLE and the poll both drive the same sweep, so they can't disagree.
- **Reconnect with backoff** when the connection drops (a laptop sleeps, wifi blips) — ordinary and
  silent, the poll covering the gap.
- **No IDLE?** The watcher notices from `CAPABILITY` and stops; that account simply relies on the poll.

## Out of scope (named)

IDLE on folders other than the INBOX (the badge and notifications are inbox-scoped anyway). Picking up
an account added *after* launch without a restart (the poll covers it meanwhile). Coalescing the wake so
only the pushed account syncs rather than a full sweep (a sweep is cheap; the sync lock and backoff
already protect it).
