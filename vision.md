# GeleitMail — Vision (the overall picture)

What the *finished* product is. This is the destination; `roadmap.md` is how we get there in
milestones, and the milestones can move as we learn — but they move *toward this*. Governed by
`constitution.md`.

---

## In one line

A native, local-first, privacy-first desktop email client for private people: add all your
accounts in a click, read and write your mail calmly and instantly, and never get tracked,
profiled, or routed through anyone's servers but your own provider's.

## Who it's for

**Private people — not power users.** Someone with 2–3 addresses (a personal Gmail, an old
GMX/Yahoo, a work Outlook) who wants them in one calm place, values speed, and distrusts being
tracked or advertised to. They will not tolerate setup friction, master-password prompts, or
clutter. Every design choice favors *clarity for a regular person* over power-user flexibility.

## The promise (honest privacy)

- **No telemetry** — nothing about how you use the app leaves your machine.
- **No middleman** — the app talks directly to your provider; we run no server your mail or
  metadata passes through.
- **No tracking** — remote content blocked by default; opening mail never phones home.
- We **never** claim "your mail never leaves your device" — the provider has it; that's email.
  We claim: *no middleman, no telemetry, no tracking.*

## The feel (the actual differentiator)

**Calm + fast.** Native, instant, quiet, uncluttered. **Effortless setup** — add an account and
your mail is just *there*. The feel is the product; there is no gimmick feature it depends on.

---

## What "finished" includes (end state)

- **Many accounts**, many providers — **both** a unified inbox *and* per-account views.
- **One-click account setup** — Gmail & Microsoft via OAuth; generic IMAP as a manual fallback.
- **Fully offline** — read *and* compose/file/flag offline, reconciled cleanly on reconnect.
- **Instant search** — fast, local, works offline, scales to large mailboxes, supports operators.
- **Safe HTML rendering** — sandboxed, remote content blocked by default, no tracking, no scripts.
- **Full composition** — reply/reply-all/forward, attachments, correct identity per account.
- **Organization** — folders/labels, archive/delete/flag/move, plus rules, snooze, notifications.
- **Transparent encryption at rest** — OS-keychain-backed; no master-password friction.
- **Cross-platform desktop** — Windows, macOS, Linux from one codebase.
- **Native throughout** — lean binaries, low RAM; the HTML renderer is the one sandboxed,
  contained exception (email is fundamentally a web document).

## Deliberately later (not part of the first finished desktop product)

- **Mobile**, then **maybe web** — desktop-first; web is in tension with local-first by design.

---

## How the vision maps to milestones

The full feature set above is reached incrementally. Notably, several capabilities arrive
*after* the first releasable product, by design — they are later milestones, not cut features:
**unified inbox**, **offline compose + reconciliation**, and **power search / rules / snooze /
notifications**. See `roadmap.md`.
