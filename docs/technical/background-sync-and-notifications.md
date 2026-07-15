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


---

## Notifications (slice 3)

### The debt, not the diff

`sync::notifiable` answered "is this news?" from **one sync's diff** against the store. That signal is
consumed by whichever writer stores the message first — and `ipc::refresh` runs a *detached backfill*
that stores messages too. Mail arriving in that window was swept up by the backfill, and no later sync
could call it new again: it sat in the inbox, unread and **never announced**. The slice-1 doc flagged
this and left it open; this closes it.

`message.notified` (migration 17) makes being told a durable fact:

| writer | owes a notification? |
| --- | --- |
| a folder's **first** sync | no — a first look is not news, or a new account notifies once per message in its inbox |
| a primed sync's new UIDs | yes, if unseen |
| the **backfill**, for old mail | no — that is what it went to fetch |
| the **backfill**, above the newest UID we already held | **yes** — that is the message the old signal lost |
| any message already `\Seen` on the server | no — it was read elsewhere |

`sync::News` is that table — **pure, and in `sync.rs` rather than `imap.rs`**, because `imap.rs` is
excluded from mutation testing: `News::All => false` (notifications never fire at all) and
`News::None => true` (a new account notifies once per message in its inbox) are mutants that would have
survived in silence. The debt is then queried from the store, not from the sync's return value — so a
message that arrived while notifications were off, or while the user was asleep, is still owed.

**Settle after, never before.** The debt is settled only once `notifier.notify()` has returned. A crash
in between costs a repeated notification; the other order costs a silently swallowed one. (`FakeNotifier
::failing()` exists so that guarantee is actually testable — a desktop whose notification service hasn't
started is the ordinary case 30 seconds after login.)

**The count comes from the store, not from the sample.** We read a handful of messages to name the
senders; the *number* is a `COUNT(*)`. Reporting the sample size would tell a user with 300 waiting that
they have 10 — and then settle only those 10, so the next sweep raises another "10 new messages" five
minutes later, about older mail. That is the storm collapsing exists to prevent, rebuilt out of the fix
for it. Settling is bounded by the id we told them about (`mark_notified_through`), so mail that lands
*while the notification is on screen* keeps its own debt.

**A UIDVALIDITY reset must not settle a debt.** The reset clears the folder and re-fetches it as
unprimed (announcing nothing — otherwise the whole inbox pops up), which would silently write off mail
we owed but hadn't raised yet. The owed `Message-ID`s are carried across the rebuild and re-owed. The
server renumbering its mailbox is not the user being told about their mail.

**Refresh settles what it fetched.** The user asked for that mail and is looking at the list it landed
in; a popup about it two minutes later is the app interrupting them about something already on their
screen. (The old diff-based signal *couldn't* do this — Refresh's own diff consumed it. Making "told" a
durable fact is what created the possibility of telling someone twice.)

### The decisions are pure

`notify.rs`: `summarize` (one message → sender + subject; several → a count and the names; above the
threshold → the count *is* the message), `clean` (strip control characters, collapse whitespace, clamp
— everything on a notification was written by a stranger, and a newline in a display name forges a
second line of the popup), `QuietHours::{parse, contains}` (wrapping midnight, which a naive
`start <= now < end` gets exactly backwards — silent all day, loud all night), and `verdict`:

- **Announce** — show it, then settle the debt.
- **Hold** (quiet hours) — say nothing, **keep** the debt. The user learns in the morning, once.
- **Drop** (switched off) — say nothing, settle it. Otherwise switching notifications on greets the
  user with every message that arrived while they were off.

A malformed quiet-hours setting parses to `None` — "no quiet hours", never "silent forever".

### The desktop, and one dependency trap

`org.freedesktop.Notifications` over D-Bus via **zbus**, which is already in the tree (the
secret-service keyring backend is built on it) — so notifications add **no new dependency**. A
`Notifier` trait keeps D-Bus out of the app and the desktop out of the tests, exactly as `SecretStore`
does for the keychain.

The trap, found the hard way: enabling zbus's `tokio` feature here switched the **keyring's** zbus onto
tokio as well (cargo unifies features across the graph), and the engine's `block_on` then panicked with
*"cannot start a runtime from within a runtime"* the first time an account was added. A dependency's
features are not local to the crate that declares them.

The D-Bus call itself runs on `spawn_blocking`: it is a synchronous connect + authenticate + call with
no timeout, and the scheduler is an async task on Tauri's runtime. A notification daemon that is slow to
start would otherwise stall a runtime worker — and if a future dependency bump ever *did* pull in
`zbus/tokio`, the panic would unwind the scheduler's loop and silently stop background sync for every
account, for the rest of the session. Blocking work belongs on a blocking thread.


---

## The unread badge (slice 4)

The window title carries the unread count (*"GeleitMail — 3 unread"*), via `WebviewWindow::set_title`
— core Tauri, no plugin, no new system dependency (a tray icon would need `libayatana-appindicator3`).

- **The count is `Store::total_inbox_unread`** — `seen = 0` across every account's `INBOX`, and only the
  inbox: mail auto-filed into a folder, or sitting in Junk, is not what the number means. `INBOX` is
  matched `COLLATE NOCASE` (IMAP reserves the name case-insensitively).
- **The text is pure** (`dto::window_title`), so it is mutation-tested: `0` (and any nonsense negative)
  is the bare name — an always-on badge is decoration; the count is capped at `999+` so it can't shove
  the app's name off a short title.
- **It is a projection of the store, never of the frontend's optimistic view**, so it cannot drift. The
  frontend calls `update_badge` (fire-and-forget) on load and after each read/mark-unread/bulk/move; the
  scheduler and `refresh` call `ipc::set_badge` host-side after a sync — which is also when a message
  read elsewhere can come back `\Seen`, so the count falls for that too, not only when mail arrives.

The badge lagging the on-screen change by a store round-trip is deliberate: it is a taskbar glance, not
a live counter, and reading truth from the store beats keeping a second tally in sync with it.


---

## Read state, both directions (SYNC-5)

A sync used to only **add and remove whole messages** — it never noticed a flag flip on one it already
held. So reading a message in webmail left it bold here, and the unread badge never fell for it; the
number reflected what you'd read *on this device*, not what you'd read anywhere.

A sync now **pulls** the server's `\Seen` / `\Flagged` for mail we already hold. Cheaply: two
`UID SEARCH`es (`SEEN`, `FLAGGED`) give the server's flagged-UID sets whatever the mailbox size — far
lighter than re-fetching every message's flags — and `sync::flag_plan` (pure, mutation-tested) keeps
only the UIDs whose stored flags actually differ. `Store::apply_flag_changes` writes just `seen` /
`flagged`, never the envelope, the body, or `notified` (a message read elsewhere settles its own
notification debt by becoming `seen`, which `pending_notifications` already filters on).

**A local change the server hasn't confirmed is never reverted.** This is the care-point, and the
review caught why it matters: reading a message marks it read *locally* and writes `\Seen` back on a
worker — and the most common read path, opening a message, had **no** server write-back at all until
this slice (it was deferred as "S9.4"). So a naive server-wins pull would flip every read straight back
off on the next sweep. The fix is a **pending-change marker** (`message.flags_dirty`, migration 18):
`set_seen`/`set_flagged` mark the flag dirty, `flags_in_folder` (the pull's local side) *excludes*
dirty rows, and the write-back worker clears the marker **only on success**. So:

- while the write-back is in flight, the message is shielded — the pull can't undo it (fixes the TOCTOU
  race a reviewer flagged);
- when it confirms, the marker clears and the next pull reconciles normally, finding agreement;
- if it never confirms (offline, server error), the marker stays and **local intent wins forever** —
  exactly the pre-SYNC-5 behaviour, preserved rather than regressed.

Opening a message now **also writes `\Seen` back** (completing S9.4), so a read here reaches the user's
other devices too. Among confirmed messages there is no per-flag modification sequence (no CONDSTORE),
so it is last-writer-wins with the server as the reconciler.

### The durable write-back queue

The write-back is no longer one fire-and-forget attempt that's lost if it fails. **The queue *is* the
`flags_dirty` rows**: a read/star made here is written locally, marked dirty, and stays dirty across
restarts until its push to the server confirms (`Store::pending_flag_writebacks` reads the queue;
`clear_flags_dirty` drains an entry). `run_flush_flags` pushes every dirty message's current flag state
— one session per folder, `+FLAGS`/`-FLAGS` per flag so other flags (`\Answered`, `\Draft`) are never
touched — and clears the marker only for the UIDs that landed.

It is drained from three places: immediately after a local change (low latency when online), by the
scheduler **every sweep**, and by Refresh. So a change made offline reaches the server the next time
we're online, rather than being lost — and because the immediate push and the sweep both go through the
same queue, a failed attempt is simply retried, never dropped. The queue and the SYNC-5 pull compose
cleanly: the sweep flushes *before* it pulls, so the pull sees a server that already agrees.

**Clear is compare-and-clear, not unconditional.** The flush reads a message's flags, pushes them over
the network, then settles the debt — and the user can change the flag again in that window (read, then
immediately unread). `clear_flags_dirty(id, seen, flagged)` therefore only clears if the flags still
match what was pushed; if they moved, it's a no-op and the row stays queued for the next flush with the
*new* value. Without this the stale confirmation would settle a debt the server doesn't reflect, and the
pull would revert the user's newer change — a silent lost update.

**Single-flight per account.** A bulk mark-read fires one local change per message; without a guard each
would spawn its own flush thread, each pushing the whole queue — O(N²) round-trips and N logins. So
`spawn_flush` coalesces: if a flush is already running for the account, it just asks it to run once more
when it finishes.

A UID the server has since dropped is counted as done (its `STORE` is a harmless tagged-OK no-op on an
RFC 3501 server like Dovecot), so a deleted message never wedges the queue — pinned by a live test. A
non-standard server that answered `NO` to that `STORE` would leave the row dirty until the sync deletes
it; not the target server, and it self-heals on the next sync's delete.

Still out of scope (named, not smuggled): the queue does not yet **surface** a persistently-failing
write-back to the user (a message stuck dirty for days), and it covers flags only — a *move* or
*delete* is server-first, so a failed one doesn't diverge and isn't queued.

The **delete-then-pull** order matters: messages the server dropped are removed *before* the pull, so a
soon-to-vanish UID is neither reconciled (a wasted write) nor counted in `flag_updates` (a spurious
re-list).

`SyncOutcome.flag_updates` reports how many held messages changed, so the scheduler re-lists the UI
(not just recomputes the badge) when flags moved even though no mail *arrived* — otherwise the badge
would fall while the list rows stayed bold until the next unrelated re-list. The live test
`a_message_read_on_another_device_stops_being_unread_here` proves the whole round trip against Dovecot.
