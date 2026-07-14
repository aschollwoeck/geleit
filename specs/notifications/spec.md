# Background sync + new-mail notifications (NOTIF-1/2/3)

**Constitution:** P1 (the UI never waits), P2 (privacy), P3 (calm + fast), P8 (spec-driven).
**Stories:** NOTIF-1 (get notified of new mail), NOTIF-2 (control per account / quiet hours),
NOTIF-3 (unread count / badge), plus the background N-account sync scheduler deferred from M7.

## Why

Today mail arrives **only when the user presses Refresh** — there is no timer, no IMAP IDLE, no
background sync. And Settings → Notifications has a toggle that persists a `notify` setting **that
nothing reads**: there is no notification code at all. For a mail client, that's the most conspicuous
gap left. The two halves belong together: mail has to arrive on its own before telling you about it
means anything.

## The milestone, in four slices

1. **New mail is knowable** ✅ — engine + store; no user-visible change.
2. **The host syncs on its own** (this slice) — the scheduler + the collision guard.
3. **Notify** — NOTIF-1 + NOTIF-2.
4. **Badge** — NOTIF-3 (unread count in the window title).

---

## Slice 1 — New mail is knowable

Three things must be true before anything can sync in the background, let alone notify. None is
user-visible; all are prerequisites.

### 1. The store must tolerate concurrent access (a latent bug, fixed here)

`Store::init` sets only `PRAGMA foreign_keys = ON` — **no WAL, no `busy_timeout`**. With the default
rollback journal and `busy_timeout = 0`, a concurrent reader and writer collide with `SQLITE_BUSY`
*immediately*, no retry. This is not hypothetical: `ipc::refresh` already spawns a **detached
backfill thread** that opens its **own** connection to the same file (the engine calls `open_store`
rather than using `AppState`'s), so it can already race a UI read today. A background scheduler would
make that routine.

→ `PRAGMA journal_mode = WAL` (readers don't block the writer) + a `busy_timeout` (a writer waits its
turn instead of failing). WAL is a property of the database file, so it survives reopens.

### 2. A sync must report **which** messages arrived, not just how many

`imap::sync_folder_incremental` computes exactly the right thing already — `sync::reconcile` gives it
`plan.new` (server UIDs absent locally) — and then returns a bare `usize` that `run_refresh`
**throws away**. That discarded value *is* the new-mail signal.

→ Widen the return to the arrived messages (uid + subject + sender + `seen`), and thread it through
`run_refresh`. No new store query, no timestamp watermark.

### 3. "New to us" is not "new to the user" — folders must be **primed**

`plan.new` means *not in our local store*, which is not the same as *new to the world*. Two cases
would otherwise fire a notification storm for mail the user already knows about:

- **A brand-new account's first sync** — the local UID set is empty, so the entire recent window is
  "new".
- **A UIDVALIDITY reset** — the server invalidated its UIDs, `sync_folder_incremental` clears the
  folder, and *everything* looks new again.

→ Priming is a **recorded fact** (`folder.primed`, set once a sync completes, cleared on a UIDVALIDITY
reset), not an inference from the folder's contents — inferring it from "has messages" would swallow
the very first message into an empty inbox. An unprimed folder syncs normally but reports **nothing**
as arrived. Priming is a correctness requirement, not polish.

Mail already read in another client is also not news: an arrival is notification-worthy only when it
is **new *and* unseen** (the `\Seen` flag comes back with the envelope).

### Pure, tested seam

Two pure functions beside `reconcile` in `sync.rs` (already pure + mutation-tested) carry the whole
"is this news?" decision, so it is testable with no clock, no network and no database:

- `should_announce(was_primed, uidvalidity_changed) -> bool` — may this folder's arrivals be announced
  at all?
- `notifiable(arrived, primed) -> Vec<&Arrived>` — of those, keep the ones not already read elsewhere.

### Out of scope (later slices)
The notification itself and its settings (slice 3); the badge (slice 4). IMAP IDLE (push) is **not** in this milestone: it needs a long-lived connection
per account per folder with re-IDLE/backoff, it doesn't remove the need for polling (servers that
don't advertise it, reconnects), and it reshapes `sync_actions` (which builds a fresh runtime and
session per call). Polling first; IDLE plugs into the same "new mail detected" seam later.


---

## Slice 2 — The host syncs on its own

A scheduler in the **Tauri host** (not the frontend: a webview throttles timers in a hidden window,
which is exactly the case this exists for, and the frontend only knows the account you're looking at).
Every 5 minutes it sweeps each account's INBOX. Failure is ordinary and **silent** — the laptop
sleeps, the wifi drops — so it backs off (5m → 10m → 20m → 30m cap, reset on success) rather than
raising a toast for a sync the user didn't start. A sweep only counts as failed if *every* account
failed, so one dead password can't stall the rest.

### The sync lock — the care-point

**Every** sync goes through `ipc::sync_folder_once`, which holds a per-`(account, folder)` async lock.
Without it the scheduler and a user-pressed Refresh would compute "what's new" from the same snapshot,
fetch the same messages, and — once slice 3 lands — **notify twice for one email**. (The UI's
`refreshing` flag could never have done this: it's per-window UI state, and the engine's workers don't
even share the store connection.) It **waits** rather than skips, so a Refresh pressed mid-sync still
ends with fresh mail on screen; it just queues.

### Arrival
`mail-arrived` fires only when something is worth announcing. The UI re-lists **quietly** — no toast,
no jump — through the same `request` epoch as every other re-list, so it can't clobber a search being
typed or a folder just switched to. It skips entirely while a search is open.
