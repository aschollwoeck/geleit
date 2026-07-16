# Background sync + new-mail notifications (NOTIF-1/2/3/4)

**Constitution:** P1 (the UI never waits), P2 (privacy), P3 (calm + fast), P8 (spec-driven).
**Stories:** NOTIF-1 (get notified of new mail), NOTIF-2 (control per account / quiet hours),
NOTIF-3 (unread count / badge), NOTIF-4 (a persistent tray icon; close keeps running in the
background), plus the background N-account sync scheduler deferred from M7.

## Why

Today mail arrives **only when the user presses Refresh** — there is no timer, no IMAP IDLE, no
background sync. And Settings → Notifications has a toggle that persists a `notify` setting **that
nothing reads**: there is no notification code at all. For a mail client, that's the most conspicuous
gap left. The two halves belong together: mail has to arrive on its own before telling you about it
means anything.

## The milestone, in four slices

1. **New mail is knowable** ✅ — engine + store; no user-visible change.
2. **The host syncs on its own** ✅ — the scheduler + the collision guard.
3. **Notify** ✅ — NOTIF-1 + NOTIF-2.
4. **Badge** ✅ — NOTIF-3 (unread count in the window title).
5. **Tray** ✅ — NOTIF-4 (a persistent tray icon; close-to-tray keeps mail arriving).

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


---

## Slice 3 — Telling the user

The **Settings → Notifications** toggle has, until now, persisted a `notify` setting that **nothing
read**. It reads now.

### Being told is a fact about the message, not about a sync

Whether mail was news used to be a diff against the store, computed by one sync — so whoever wrote the
message first ate the signal. A message that arrived while the **backfill** thread was running got
stored by the backfill, was therefore no longer "absent from our store", and **no later sync could ever
call it new**: it sat in the inbox, unread and unannounced, forever.

So `message.notified` (migration 17) is a durable debt. Every writer records it: a folder's first sync
owes nothing (a first look is not news), the backfill owes nothing for the **old** mail it exists to
fetch — but everything above the newest UID we already held is still owed, which is exactly the message
the old signal lost. A message already `\Seen` on the server was read elsewhere and is never news, and
reading it here settles the debt before the notification is ever raised.

The debt is settled **after** the notification is raised, never before: a crash between the two costs a
repeated notification, and the other order costs a swallowed one. Only one of those loses mail.

### Calm by construction (P3)

- **Several messages are one notification.** "3 new messages — From Alice, Bob, Cara"; above the
  threshold, the count *is* the message. A popup per message is how a mail client teaches you to switch
  its notifications off.
- **Quiet hours** hold the debt rather than dropping it: the mail is still there in the morning, and so
  is one notification saying so. Switching notifications **off**, by contrast, drops the debt — else
  turning them back on would greet the user with everything they missed.
- **Per account**, because one noisy mailbox shouldn't cost the notifications of the other.

### Everything on a notification was written by a stranger

The sender chose their own display name; the subject is theirs too. A newline in a display name lets
them forge what looks like a second line of the popup ("Alice Baker\nYour password has expired"), and
the desktop renders it faithfully. So every field is stripped of control characters, has its whitespace
collapsed, and is clamped — a subject long enough to push the real content off the screen is not an
accident either.

### The desktop

`org.freedesktop.Notifications` over D-Bus, through **zbus** — already in the tree (the secret-service
keyring is built on it), so this costs **no new dependency**. A [`Notifier`] trait keeps D-Bus out of
the app and lets the tests run without a desktop, exactly as `SecretStore` does for the keychain.


---

## Slice 4 — The badge

The window title carries the unread count — *"GeleitMail — 3 unread"* — so a glance at the titlebar or
the taskbar says whether anything is waiting. Zero unread is the bare name: a badge that is always on
is decoration, not a signal. Capped at `999+`, so an enormous inbox can't push the app's own name out
of a short title.

**Inbox-scoped, across every account.** Mail a server-side rule filed straight into a folder, and
anything in Junk, is not what "you have unread mail" means to someone glancing at the taskbar. The
count is `Store::total_inbox_unread` — one query over every account's `INBOX`.

**The title is a projection of the store.** `dto::window_title(n)` is pure (mutation-tested); the
count comes from the store, never from the frontend's optimistic view — so the badge can't drift from
the truth. It is recomputed after anything that changes what's unread: the frontend fires it on load
and after a read / mark-unread / bulk / move (a store round-trip behind the on-screen change, which a
taskbar glance can afford), and the scheduler resets it after every sweep — which is also when mail
read on another device could have come back, so the number falls for that too.

The badge shipped without a tray icon: that needs `libayatana-appindicator3`, a new system dependency,
and `WebviewWindow::set_title` (core Tauri) already puts the number in the taskbar. The tray was left as
its own slice — now taken up below.

---

## Slice 5 — The tray (NOTIF-4)

A persistent **system-tray icon**, so GeleitMail keeps running — and keeps checking mail — after the
window is closed. This is the piece that makes "mail arrives on its own" actually reach a user who has
closed the window: a webview-hosted window that's *closed* is a dead process; a *hidden* one behind a
tray icon is not.

- **Closing hides, it doesn't quit.** The window's `CloseRequested` is intercepted (`api.prevent_close`)
  and the window is hidden. The scheduler and IDLE watchers keep running, so mail still lands and the
  count still updates. **Quit** (the tray menu) is the only real exit.
- **The icon reveals the window.** The **Show GeleitMail** menu item shows, un-minimises, and focuses
  it. On Linux the app-indicator opens its menu on any click and emits no click events of its own, so
  the menu is the way back there; a direct left-click-to-restore is wired for macOS/Windows, where it
  works, and is inert on Linux.
- **The tooltip mirrors the badge.** `ipc::set_badge` is already the one chokepoint that writes the
  unread count to the window title; it now also writes it to the tray tooltip (`tray_by_id`), so the two
  can't disagree. Hovering the icon reads *"GeleitMail — 3 unread"* even while the window is hidden.

**The cost, accepted deliberately.** The `tray-icon`/`muda` crates were already in the lock graph, so no
new *Rust* dependency — but the feature links `libayatana-appindicator3`, the project's **first non-Rust
system dependency** (added to the README prerequisites). That was the explicit trade the badge slice
declined and this one takes. `tray.rs` is glue over a live `AppHandle` + a desktop tray, so it's
mutants-excluded like `idle.rs`; there's no pure logic here to split out — the count formatting it reuses
(`dto::window_title`) stays mutation-tested.
