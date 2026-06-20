# GeleitMail — Roadmap (milestones & slices)

How we get from nothing to the `vision.md` end state, governed by `constitution.md`.

**This is a living document.** Milestones are outcomes; **slices** are thin, end-to-end,
verifiable steps toward a milestone — each slice should leave the project working. We expect
to re-plan at every milestone boundary. Slices for *distant* milestones are provisional
sketches. Per constitution P8, each milestone gets its own **spec → plan → tasks**, written
just before we build it:

```
specs/m0/spec.md    plan.md    tasks.md   # what · how · done-vs-todo (kept current)
specs/m1/...
```

**Build philosophy:** engine-first (the make-or-break core before any UI commitment),
de-risk early (feasibility before foundations), each milestone a capability you can actually
verify. Single-account, plain-IMAP development first so we are never *blocked* on OAuth
approval — but the OAuth paperwork starts as a background track at M0 (weeks of lead time).

---

## M0 — Foundations & feasibility
**Outcome:** we commit to the native UI stack *with evidence*, on a working scaffold — or we
pivot before building on sand.

- **S0.1** Cargo workspace scaffold; crate skeleton; CI with fmt + clippy + test + `cargo mutants` wired.
- **S0.2** Spike: render a real-world HTML email in a *sandboxed* component, remote content blocked.
- **S0.3** Spike: virtualized message list rendering ~50k synthetic rows at 60fps in Slint.
- **S0.4** Decision record (ADR): commit to Slint, or pivot — based on S0.2/S0.3 evidence.
- **S0.5** *(parallel admin track)* begin Google + Microsoft OAuth app-registration paperwork.

## M1 — Core engine (headless)
**Outcome:** a headless tool that correctly syncs a real mailbox into an encrypted local store —
the make-or-break core, proven, with no UI.

- **S1.1** Local store schema (account-scoped from day one) + SQLite + migrations.
- **S1.2** Transparent encryption at rest via OS keychain (no master password).
- **S1.3** Connect to one IMAP account (plain / app-password); list folders.
- **S1.4** Sync message envelopes/headers for a folder, newest-first.
- **S1.5** Progressive backfill of older messages in the background, batched.
- **S1.6** Fetch full bodies + MIME parse (`mail-parser`) + store.
- **S1.7** Incremental sync (CONDSTORE/QRESYNC): detect new / changed / deleted since last sync.
- **S1.8** Gmail-specific handling (labels-as-folders, X-GM-EXT-1).
- **S1.9** Sync integrity: resume after interruption, idempotent, provably no dupes/loss.

## M2 — Local search
**Outcome:** fast queries against the synced store, offline.

- **S2.1** Index synced messages into `tantivy`.
- **S2.2** Incremental indexing as new mail arrives / changes.
- **S2.3** Query API (sender / subject / body), fast.
- **S2.4** Verified to work fully offline against the local index.

## M3 — Read it (single-account UI)
**Outcome:** you can actually read your mail in GeleitMail — calm and instant.

- **S3.1** Slint app shell (folder list · message list · reading pane) reading the local store only.
- **S3.2** Virtualized message list, newest-first, read/unread state.
- **S3.3** Reading pane: display a selected message (plain text first).
- **S3.4** Folder navigation.
- **S3.5** Offline reading verified; non-blocking background-sync indicator.
- **S3.6** Calm/fast pass: interactions instant, nothing blocks on the network.

## M4 — Safe rendering
**Outcome:** HTML mail renders safely; nothing phones home.

- **S4.1** Integrate the sandboxed HTML renderer (from S0.2) into the reading pane.
- **S4.2** Block remote content by default; neutralize tracking.
- **S4.3** Per-message "load remote content" opt-in (+ optional "blocked N trackers" cue).
- **S4.4** Security hardening: no script execution, no outbound requests, sandbox-escape tests.

## M5 — Send it
**Outcome:** full read + send for one account.

- **S5.1** SMTP send (`lettre`).
- **S5.2** Compose window (new message).
- **S5.3** Reply / reply-all / forward (correct quoting; from the current account).
- **S5.4** MIME build (`mail-builder`): attachments, correct encoding.
- **S5.5** Outbox + Sent handling; retry on failure.

## M6 — Organize it
**Outcome:** manage your inbox, consistent with the server.

- **S6.1** Local actions: archive, delete, read/unread, star, move.
- **S6.2** Write-back sync: apply local actions to the server (IMAP).
- **S6.3** Reconcile server-side changes; stay consistent (no dupes/loss).
- **S6.4** Optimistic UI: instant locally, syncs in background, handles failures gracefully.

## M7 — Many accounts + OAuth
**Outcome:** the effortless-setup hook is real — add accounts in a click, switch between them.

- **S7.1** Gmail OAuth (loopback redirect; tokens in keychain).
- **S7.2** Microsoft OAuth (gated on S0.5 approval).
- **S7.3** Multiple accounts in store; sync scheduler handles N accounts concurrently.
- **S7.4** Per-account switcher UI.
- **S7.5** One-click add-account onboarding flow.
- **S7.6** Generic IMAP manual-setup fallback.

## M8 — Release
**Outcome:** the first releasable GeleitMail.

- **S8.1** Encryption hardening + security review of crypto / OAuth / HTML paths.
- **S8.2** Cross-platform builds + installers (Windows, macOS, Linux).
- **S8.3** First-run/onboarding polish; error and edge-case states.
- **S8.4** Final performance/calm pass (RAM, startup, large mailbox).
- **S8.5** Beta with real accounts; fix; tag first release.

---

## Beyond the first release (toward the full vision)
Slices defined when we reach them.

- **M9 — Unified inbox** (merged cross-account view; correct from-address on reply).
- **M10 — Offline compose + reconciliation** (compose/file/flag offline, merge on reconnect).
- **M11 — Power features** (instant search at scale + operators, rules/filters, snooze, notifications).
- **Later** — mobile, then maybe web.
