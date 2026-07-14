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

---

## The scheduler (slice 2)

`scheduler.rs`, spawned in `main`'s `setup`. Every 5 minutes it sweeps every account's INBOX. It lives
in the **host**, not the frontend, for two reasons that both matter: a webview **throttles or freezes
timers in a hidden or occluded window** — which is precisely the situation this feature exists for —
and the frontend only knows the account you are *looking at*, while the host can just ask the store for
all of them.

- **Failure is ordinary and silent.** The laptop sleeps; the wifi drops. A failed sweep never raises a
  toast — an error the user didn't ask for, about a sync they didn't start, is noise. It backs off
  instead: 5m → 10m → 20m → 30m (cap), reset by one success. `backoff()` is pure and unit-tested.
- **One bad account doesn't stop the others.** A sweep is only a failure if *every* account failed
  (which almost certainly means it's us — we're offline), so a single dead password can't push the
  whole schedule into backoff.
- **A failing account is tried progressively less often** (every 2nd, 4th, then 8th sweep — capped, so
  a fixed password recovers on its own). "Unreachable" and "wrong password" look identical from here
  but behave very differently, and a client that retries a revoked login every five minutes for days,
  unattended, is how a provider decides to lock the account or block the IP.
- **A successful Refresh wakes it.** `tokio::time::sleep` is monotonic — a suspended laptop doesn't
  burn the wait down while it's asleep — and a machine that was offline overnight has backed off to the
  half-hour cap. So mail would be up to 30 minutes stale exactly when the user sits down. A
  user-pressed Refresh that *succeeds* is the strongest evidence we have that the network is back, so
  it pokes the scheduler, which resets and sweeps at once.
- **`GELEIT_SYNC_SECS=<n>`** (debug builds only) replaces the interval **and the backoff**, because a
  5-minute poll cannot be watched by hand. That's how this was verified end to end: run the app
  against the local Dovecot, deliver an unseen message from *outside* the app, and watch it appear in
  the list — untouched. (It bypasses backoff, so backoff itself is exactly the path the dev seam
  can't exercise — which is why it's unit-tested instead.)

The decisions — `backoff`, `sweep_verdict`, `should_try` — are **pure and unit-tested** in
`schedule.rs`; `scheduler.rs` is the glue that acts on them, and is excluded from mutants (it needs a
live `AppHandle`, an IMAP server and a clock).

### The sync lock — the care-point of this milestone

**Every** sync — the scheduler's and a user-pressed Refresh alike — goes through
`ipc::sync_folder_once`, which takes a per-`(account, folder)` async lock (`AppState::sync_lock`).

Without it, both paths would compute "what's new" from the same local snapshot, both would fetch the
same messages, and (once slice 3 lands) the user would get **two notifications for one email**. Note
the UI's `refreshing` flag could never have done this job: it's per-window UI state, and the engine's
workers don't even share `AppState`'s store connection.

It **waits** rather than skips: a Refresh pressed during a background sync queues behind it and then
syncs again — finding nothing new, because the first sync already stored it. That's the right shape.
The user's click still ends with fresh mail on screen; it just doesn't race.

### Arrival, in the UI

The scheduler emits `mail-arrived` only when something is actually worth announcing. The UI re-lists
**quietly** — no toast, no scroll jump: the message simply appears with its unread dot, the way mail
should. The re-list goes through the same `request` epoch as every other one, so it cannot clobber a
search the user is mid-way through typing or a folder they just switched to. It skips entirely while a
search is showing results, or while the drafts pane is open. An open message stays open.

## Deliberately not IMAP IDLE (yet)

`async-imap` does support IDLE. But it means a long-lived TLS connection **per account per folder**,
re-IDLE every ≤29 minutes (RFC 2177), reconnect/backoff on drop, and it reshapes `sync_actions` —
which today builds a fresh runtime and a fresh session per call. It also doesn't remove polling
(servers that don't advertise IDLE; reconnects). Polling first; IDLE plugs into this same
"new mail detected" seam later.
