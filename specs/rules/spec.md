# Auto-sort with rules / filters (ORG-8)

**Constitution:** P1 (the UI never waits), P2 (local-first, private), P3 (calm), P8 (spec-driven).
**Story:** ORG-8 — auto-sort incoming mail with rules.

## Why

A busy inbox wants triage you don't have to do by hand: newsletters to a folder, a mailing list marked
read, mail from one person starred. A **rule** is *when this, do that* — and running it for you is the
difference between an inbox that tidies itself and one you shovel every morning.

## Client-side, by decision

Rules run **inside GeleitMail, on this device, when it syncs** — not on the provider. That's the
maintainer's call ([the alternative](../../roadmap.md), server-side Sieve, needs ManageSieve support,
capability detection, and a script compiler, and doesn't fit the local-first architecture). The trade,
named plainly: a rule fires while GeleitMail is checking mail, **not while the app is closed** — the
periodic sync (and IMAP IDLE) is when new mail is processed. A **Run on inbox now** action covers "apply
this to what's already here".

## A rule

*One condition, one or more actions.* Kept deliberately small for a first cut:

- **Condition** — a field (**From** / **Subject** / **To**) *contains* some text (case-insensitive). One
  condition per rule; `From` matches the sender's name or address.
- **Actions** (at least one): **move to a folder**, **mark as read**, **star**. A move takes the message
  out of the inbox into a folder you already have.

Rules are per-account and evaluated **in order**; the **first** rule a message matches wins (predictable,
and it avoids two rules fighting over where a message goes). A message a rule *moves* leaves the inbox;
mark-read / star without a move keep it there, changed.

## Applied via a durable marker (mirrors `notified`)

New mail must be filtered exactly once, and that fact has to survive a crash — the same problem
`notified` (NOTIF-1) already solved. So: `message.filtered` (migration 21). Genuinely-new mail arrives
`filtered = 0` (owed a pass); backfill and everything already in the box at upgrade is `filtered = 1`
(rules are **not** applied retroactively — enabling a rule doesn't silently rearrange old mail; that's
what **Run on inbox now** is for, opt-in). After a sync, `apply_rules` walks the account's
`filtered = 0` INBOX mail:

- no rule matches → mark it `filtered = 1` (evaluated, don't re-check);
- a rule matches → apply its flag actions (mark-read / star), then its move if any (server-first
  `run_move`, then drop the local row). Only after the whole action set lands is the message done; a
  move that fails offline leaves `filtered = 0`, so the next sync retries — and re-applying idempotent
  flag actions on retry is harmless.

`apply_rules` runs in **two passes**: it applies every matched flag change locally, **pushes those flags
to the server** (`run_flush_flags`, synchronous/best-effort), and only *then* does the moves. That order
is load-bearing: an IMAP `MOVE` carries the message's current flags, so `\Seen`/`\Flagged` must reach the
server copy *before* the move, or a "move + mark read" rule would file the message still unread — and the
row's deletion after the move would take the deferred write-back with it. On the `Moved` path the row is
marked `filtered` *and* deleted, so a delete that somehow fails still can't loop.

`apply_rules` runs on a worker (the move is network); the scheduler calls it each sweep after the INBOX
sync, and the **Run on inbox now** command (which first resets the inbox to `filtered = 0`) calls the
same code.

## Store

- `rule` table (migration 21): `account_id`, `field` (`from`/`subject`/`to`), `pattern`,
  `target_folder` (nullable folder name), `mark_read`, `star`, `position` (evaluation order), `created_at`.
- `add_rule`, `list_rules(account_id)` (by `position`), `delete_rule(id)`.
- `unfiltered_inbox(account_id)` → `(id, from_name, from_addr, subject, to_addrs)` for the pass;
  `mark_filtered(ids)`; `reset_inbox_filtered(account_id)` for **Run now**.
- `upsert_message` sets `filtered = !owed_notification` (new mail owed a pass; backfill already done).

## Core (pure, mutation-tested)

`geleit_core::rule`: `RuleField {From, Subject, To}` (+ `key`/`from_key`) and
`matches(field, pattern, from_name, from_addr, subject, to) -> bool` — case-insensitive substring; `From`
tests name and address. No I/O, no deps.

## UI

A **Rules** screen (from Settings): the ordered list of rules, each shown as a sentence
(*"If From contains **newsletter** → move to **Reading**, mark read"*) with a delete; an **Add rule**
form (field, contains-text, folder picker + mark-read + star checkboxes); and **Run on inbox now**.

## Out of scope (named)

Server-side/Sieve rules (run-while-closed); multiple conditions or any/all logic; regex; actions beyond
move/read/star (forward, auto-reply, delete); rules on folders other than the INBOX. *(Reordering — the
`↑ ↓` priority controls, `move_rule` — was a follow-up, now shipped.)*

## Acceptance criteria

1. `fmt` / `clippy -D warnings` / test / `cargo deny check` / boundary all green; `core::rule::matches`
   and the store CRUD mutants 0-missed; `perf-budget` unaffected.
2. `matches` is correct and case-insensitive across From(name+addr)/Subject/To — unit-tested.
3. Store: a new message is `filtered = 0`, backfill `filtered = 1`; `unfiltered_inbox` / `mark_filtered`
   / `reset_inbox_filtered` behave — tested.
4. `apply_rules` first-match-wins, applies flags + move, marks filtered, and retries a failed move —
   verified (store-level for match/mark; the move path live against Dovecot).
5. Add a rule, see new matching mail sorted, and **Run on inbox now** sort existing mail — maintainer's
   eyeball.
