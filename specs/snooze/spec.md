# Snooze a message (ORG-9)

**Constitution:** P1 (the UI never waits), P2 (privacy — local-first), P3 (calm + fast), P8 (spec-driven).
**Story:** ORG-9 — snooze a message: hide it now, have it come back later.

## Why

A read inbox is a to-do list you can't reorder. Snooze is the one move that lets you *defer*: "not now —
remind me tonight / tomorrow / next week." Mail you can't act on yet leaves the list instead of scrolling
away unhandled, and returns, announced, when you asked for it.

## The shape: local-first, no server mutation

Snooze is a **local** property of a message — a timestamp, `snoozed_until`. Nothing is moved on the
server. While that time is in the future the message is **hidden** from its folder list and left out of
the unread badge; when it passes, the message **resurfaces** — reappears in the list and is re-announced,
exactly as if it had just arrived.

This is deliberately device-local for now: snoozing here doesn't hide the message in webmail or on a
phone. A server-synced snooze (move to a `Snoozed` folder and back) is a larger, network-bound feature
and a named follow-up — this slice keeps the whole path local, so it can't fail offline and needs no live
server to be correct.

## Store (migration 20)

`ALTER TABLE message ADD COLUMN snoozed_until INTEGER` — NULL = not snoozed; a unix timestamp = hidden
until then. The "is it still snoozed?" test uses **SQLite's own clock** (`unixepoch()`) inside each
query, so no `now` has to be threaded through the read methods and the display can never lag the wall
clock waiting for a sweep.

- `snooze_message(id, until)` / `snooze_messages(ids, until)` — set it.
- `unsnooze_message(id)` — clear it (bring the message back now).
- `snoozed_messages(account_id)` — the ones still in the future, soonest-first, for the Snoozed view.
- `resurface_due_snoozes()` — `snoozed_until <= now`: clear it **and** set `notified = 0`, so a
  resurfaced message re-enters the existing notification pipeline (NOTIF-1) and is announced again.
  Returns how many, so the scheduler knows the list is stale.
- **Exclusion**: `messages_in_folder`, `total_inbox_unread`, and `folder_unread_counts` gain
  `AND (snoozed_until IS NULL OR snoozed_until <= unixepoch())` — a still-snoozed message is neither
  listed nor counted, in any folder or the badge.

## Resurfacing — reuses the scheduler, not a new timer

The background scheduler already sweeps (every few minutes, and on an IDLE poke) and already announces
owed mail from the store. The sweep gains one line at the top: `resurface_due_snoozes()`, run **before**
the per-account announce and **independent of connectivity** (a snooze coming back is a local event — it
must fire even when every account is unreachable). A resurfaced message has `notified = 0`, so the
existing `announce` picks it up; and the resurface count marks the sweep "changed", so the badge updates
and the UI re-lists. Granularity is the sweep interval (minutes) — fine for "remind me tomorrow", and
while the app is closed nothing resurfaces until the next launch's first sweep (named limitation).

## Preset times — computed in the host, in the user's timezone

The offered times are computed in `geleit-app` from `chrono::Local::now()` (the same local clock quiet
hours uses), so "tomorrow at 8" means the user's 8, not UTC. `snooze::presets(now)` is **pure** (unit- +
mutation-tested) and returns only the presets still in the future:

- **Later today** — now + 3 h.
- **This evening** — today 18:00 (dropped once it's past).
- **Tomorrow** — tomorrow 08:00.
- **This weekend** — the coming Saturday 08:00 (dropped on Sat/Sun).
- **Next week** — the coming Monday 08:00.

Each is `(label, unix-timestamp)`; the UI shows the labels and passes the chosen timestamp straight to
`snooze_message`. A free-form date/time picker is out of scope for this slice.

## UI

- **Snooze action** on a message (reading pane) and on a bulk selection (ORG-7), opening a small menu of
  the preset labels. Choosing one snoozes and the message leaves the list immediately (optimistic), the
  badge falls, and a toast confirms *"Snoozed until Tomorrow"*.
- **A Snoozed view** in the rail (mirroring Drafts/Outbox): each row shows sender, subject, and when it
  comes back; an **Un-snooze** action brings it back now.

## Out of scope (named)

Server-synced snooze (a `Snoozed` folder that hides the mail across devices); a custom date/time picker;
per-message "resurface at the top / keep read state" niceties; snoozing whole threads as a unit.

## Acceptance criteria

1. `fmt` / `clippy -D warnings` (+`dangerous-tls`) / test / `cargo deny check` / boundary check all green;
   store mutants on the new pure logic 0-missed; `perf-budget` unaffected.
2. A snoozed message is hidden from its folder and the badge until its time, then resurfaces and is
   announced — proven by store tests (exclusion + `resurface_due_snoozes`) and a scheduler test.
3. `snooze::presets` returns the right local timestamps and drops past presets — unit-tested.
4. The Snooze menu, the Snoozed view, and un-snooze work end to end — the maintainer's eyeball on the
   running app.
