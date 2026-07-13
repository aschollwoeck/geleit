# Background sync & new-mail notifications

How mail arrives on its own, and how the user gets told. Spec: `specs/notifications/spec.md`.

Built in four slices — this document grows with them. Slice 1 (below) is the groundwork: no
user-visible change, but nothing above it is safe without it.

---

## The store must survive concurrent access (fixed in slice 1)

**GeleitMail runs several SQLite connections against one file.** The IPC commands share one (behind
`AppState`'s `Mutex`), but the engine's workers do **not** use it — `run_refresh`, `run_backfill`, and
(from slice 2) the scheduler each call `localstore::open_store` and get their *own* connection. So the
`Mutex` in `AppState` does not serialize them, and never did.

`Store::init` used to set only `PRAGMA foreign_keys = ON`. With SQLite's default rollback journal and
`busy_timeout = 0`, a reader and a writer on different connections collide and the loser fails
**immediately** with `SQLITE_BUSY` — no retry, no wait. That is not theoretical: `ipc::refresh`
already spawns a *detached backfill thread* with its own connection, so a background write landing
while the user scrolls the list could already fail. It's a latent bug that shipped.

So `Store::init` now sets:

```
PRAGMA journal_mode = WAL;   -- readers don't block the writer, and vice versa
PRAGMA busy_timeout = 5000;  -- a second writer waits its turn instead of erroring
```

WAL is a property of the database *file*, so it persists across opens (and applies to the SQLCipher
database just the same). `a_second_connection_can_write_while_the_first_reads` pins it: it parks a
read cursor mid-iteration on one connection and writes from another. **Revert WAL and that test fails
with `DatabaseBusy — "database is locked"`** — exactly the bug it exists to prevent.

The timeout needs its own assertion, and has one: under WAL a reader never blocks a writer, so *that*
scenario can't exercise `busy_timeout` at all — drop the pragma and the test would still pass. The
timeout is what saves **writer vs writer**, which two accounts syncing at once will do routinely, so
the test reads both pragmas back on both connections (WAL is a property of the file; `busy_timeout` is
per-connection, so every connection must set it).

## Knowing *which* mail arrived — and when to shut up about it

`imap::sync_folder_incremental` already computed the right thing: `sync::reconcile` hands it
`plan.new` (server UIDs absent from our store). It then returned a bare `usize`, and `run_refresh`
**threw that away**. That discarded value *was* the new-mail signal.

It now returns a `SyncOutcome { arrived: Vec<Arrived>, primed: bool }`, where `Arrived` carries what a
notification needs (uid, sender, subject, `seen`).

### Priming — the part that is easy to get wrong

**"New to us" is not "new to the user."** `plan.new` means *absent from the local store*, and there
are two situations where that is the entire recent window:

- **A brand-new account's first sync** — the local UID set is empty, so every message is "new".
- **A UIDVALIDITY reset** — the server invalidated its UIDs, the folder gets cleared, and everything
  looks new again.

Announcing either would mean a notification per message in the inbox. So **priming is a recorded
fact, not a guess**: `folder.primed` (migration 14) is set once a sync *completes*, and cleared again
if the server resets UIDVALIDITY. An unprimed folder syncs normally and announces **nothing**.

It is deliberately not inferred from "does the folder have messages?" — that seems equivalent and
isn't: a folder that was **empty** at its first sync would never be primed, so the very first message
to arrive in it would be swallowed. And because the flag is only set *after* the sync finishes, a sync
that dies half-way doesn't leave the folder falsely primed.

Mail that was already read in another client is not news either — the `\Seen` flag comes back with the
envelope. So:

```rust
sync::notifiable(arrived, primed) -> Vec<&Arrived>   // pure, unit-tested
// = nothing if !primed; otherwise the arrivals that are !seen
```

The whole "should this be announced" decision is pure: no clock, no network, no database. The live
Dovecot test `live_new_mail_is_detected_and_first_sync_is_silent` proves the chain end to end — first
sync silent, a genuinely new message announced with its sender and subject, one already `\Seen` on the
server ignored, an idle re-sync silent.

### The signal is derived, so any other writer can consume it

`arrived` is a **diff against the store**, with no durable marker — so whoever writes the message
first eats the signal. Two consequences, both left open here and closed in the slices that can
actually act on them:

- **A body-fetch failure must not throw it away.** The envelopes are already committed, so returning
  `?` from the body fetch would discard `arrived` — and those UIDs are now local, so no later sync
  would ever call them new again: the mail would sit in the inbox, silently, never announced. The body
  fetch is therefore **best-effort** (a missing body self-heals on the next sync; a missed
  notification never does).
- **The backfill thread and overlapping syncs.** `ipc::refresh` runs a detached backfill that also
  stores messages; mail arriving in that window is swept up by the backfill and would never be
  announced. And two syncs of one folder (the scheduler and a user-pressed Refresh) both compute the
  same `arrived` and would notify **twice**.

  → Slice 2 adds the in-flight guard (kills the double-notify), and slice 3 makes "announced" a
  durable fact per message rather than a property of one sync's diff (kills the backfill window). Both
  are noted here so neither is quietly forgotten.

## Deliberately not IMAP IDLE (yet)

`async-imap` does support IDLE. But it means a long-lived TLS connection **per account per folder**,
re-IDLE every ≤29 minutes (RFC 2177), reconnect/backoff on drop, and it reshapes `sync_actions` —
which today builds a fresh runtime and a fresh session per call. It also doesn't remove polling
(servers that don't advertise IDLE; reconnects). Polling first; IDLE plugs into this same
"new mail detected" seam later.
