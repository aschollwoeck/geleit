# GeleitMail — Roadmap (milestones & slices)

How we get from nothing to the `vision.md` end state, delivering the `stories.md` catalog,
governed by `constitution.md`.

**This is a living document.** Milestones are outcomes; **slices** are thin, end-to-end,
verifiable steps toward a milestone — each slice should leave the project working. Milestones
are **derived from the user stories** (`stories.md`): each lists the story IDs it delivers, so
coverage is checkable. We expect to re-plan at every milestone boundary; slices for *distant*
milestones are provisional. Per constitution P8, **each slice** gets its own
**spec → plan → tasks**, written just before we build it; decisions that span slices are
recorded as ADRs (`docs/adr/`):

```
specs/m0/s0.1-scaffold/{spec,plan,tasks}.md
specs/m0/s0.2-html-spike/{spec,plan,tasks}.md
docs/adr/NNNN-title.md                        # cross-slice decisions
```

**Build philosophy: vertical-slice-first.** Get a thin, usable path working end-to-end early
(read one account in M1), *then* harden the engine (M2) and add breadth. This de-risks
*integration* early and lets the real experience validate the design — rather than completing
a headless engine before anything is visible. The make-or-break sync core is de-risked by
designing its schema and sync model up front (M1 plan) and proving it via the slice, then
hardening it in M2. UI framework is committed in M0 (via spikes).

**Cross-platform:** primary dev OS is **Linux**; OS-divergent components (keychain, HTML
renderer, OAuth loopback) sit behind platform-abstraction seams from M0, with full
Windows/macOS/Linux packaging and validation at M8.

**Provider auth note:** Microsoft basic auth is retired, so real Outlook accounts only work
once OAuth lands (M7). Early milestones develop against a local IMAP server or a Gmail
app-password account; OAuth app-registration paperwork starts at M0 (weeks of lead time).

---

## M0 — Foundations & feasibility
**Outcome:** commit to the native UI stack *with evidence*, on a working scaffold — or pivot
before building on sand. *(Infrastructure — delivers no user stories directly.)*

- **S0.1** Cargo workspace scaffold; UI-agnostic engine crates vs. a UI crate; CI with
  `fmt --check` + `clippy -D warnings` + tests + `cargo mutants` wired.
- **S0.2** Spike: render a real-world HTML email in a *sandboxed* component, remote content blocked.
- **S0.3** Spike: virtualized message list rendering ~50k synthetic rows at 60fps in Slint.
- **S0.4** ADR: commit to Slint, or pivot — based on S0.2/S0.3 evidence.
- **S0.5** Platform-abstraction seams for keychain / HTML render / OAuth; Linux as primary dev OS.
- **S0.6** *(parallel admin track)* begin Google + Microsoft OAuth app-registration paperwork.

## M1 — Thin slice: read one account
**Delivers:** ACC-3, ACC-4*, SYNC-1*, SYNC-2, READ-1, READ-2, READ-3, READ-6, READ-7, SEC-2.
**Outcome:** open the app, connect one IMAP account, see your folders and message list, read a
message in plaintext, and refresh — the whole stack proven end-to-end.

- **S1.1** **Visual design language** → `design.md` (type, color/theme tokens for light+dark,
  spacing/density, layout & navigation shape, component look, iconography, motion). Defined
  before any UI is built; refined for rich content in M3. The canonical "what it looks like"
  (a top-level artifact, governed by the constitution; UI slice specs cite it + guidelines §13).
- **S1.2** **First-dependency setup:** supply-chain CI (`cargo deny`/`audit`, guidelines §6) +
  adopt `thiserror` (migrate the hand-rolled errors in `geleit-platform`). Establishes the
  dependency gate *before* real deps (rusqlite, async-imap, …) start landing. (Deferred here
  from S0.2/S0.5 per plan.)
- **S1.3** Local store schema (account-scoped from day one) + SQLite (`rusqlite`) + migrations.
- **S1.4** Connect to one IMAP account via manual config (ACC-3); credentials in OS keychain
  (SEC-2); list folders (READ-6).
- **S1.5** Naive sync of a folder's recent envelopes into the store (SYNC-1 basic; ACC-4 partial).
- **S1.6** Fetch + MIME-parse (`mail-parser`) plaintext bodies into the store.
- **S1.7** Minimal Slint shell built to `design.md`: folder list + virtualized message list
  (READ-1, READ-2), reading the local store only.
- **S1.8** Reading pane: open a message in plaintext (READ-3); mark read/unread (READ-7).
- **S1.9** Manual refresh action (SYNC-2).

## M2 — Robust engine & store
**Delivers:** SYNC-3, SYNC-4, SYNC-1†, SEC-1, SEC-3, OFF-1.
**Outcome:** correct, robust, encrypted local sync — now that we can see what we're building.

- **S2.1** Encryption at rest via OS-keychain-held key (SEC-1); transparent unlock, no master password.
- **S2.2** Incremental sync (CONDSTORE/QRESYNC): detect new / changed / deleted (SYNC-1 robust).
- **S2.3** Progressive backfill of the full mailbox, newest-first, batched, in background (SYNC-3).
- **S2.4** Gmail-specific handling (labels-as-folders, X-GM-EXT-1).
- **S2.5** Non-blocking sync status / progress (SYNC-4).
- **S2.6** Sync integrity: resume after interruption, idempotent, provably no dupes/loss (property tests).
- **S2.7** Offline reading verified (OFF-1); wipe local data on account removal (SEC-3).

## M3 — Rich, safe reading
**Delivers:** READ-4, READ-5, READ-8, PRIV-1, PRIV-2, PRIV-3, PRIV-4.
**Outcome:** read real HTML mail safely, in threads, with attachments.

- **S3.1** Integrate the sandboxed HTML renderer (from S0.2) into the reading pane (READ-4).
- **S3.2** Block remote content by default (PRIV-1); no script execution (PRIV-4); hardening +
  sandbox-escape tests.
- **S3.3** Per-message / trusted-sender "load remote content" (PRIV-2); "trackers blocked" cue (PRIV-3).
- **S3.4** Conversation threading — group messages into threads (READ-5).
- **S3.5** Attachments: view and save (READ-8).

## M4 — Send
**Delivers:** SEND-1…SEND-9, ACC-7.
**Outcome:** full compose / reply / forward for one account.

- **S4.1** SMTP send (`lettre`); Sent-folder handling (SEND-8); outbox + retry.
- **S4.2** Compose window — new message (SEND-1); MIME build (`mail-builder`).
- **S4.3** Reply / reply-all / forward with correct quoting (SEND-2, SEND-3).
- **S4.4** Attachments in compose (SEND-4).
- **S4.5** Drafts: save and resume (SEND-5).
- **S4.6** Basic formatting — bold, lists, links (SEND-6).
- **S4.7** Per-account display name + signature, auto-included (SEND-7, ACC-7).
- **S4.8** Address autocomplete from history (SEND-9).

## M5 — Organize
**Delivers:** ORG-1…ORG-7, SYNC-5.
**Outcome:** manage your inbox, consistent with the server.

- **S5.1** Local actions with optimistic UI: archive, delete→trash, move, star (ORG-1…ORG-4).
- **S5.2** Write-back sync of actions to the server (SYNC-5); reconcile, no dupes/loss.
- **S5.3** Empty trash / delete permanently (ORG-2).
- **S5.4** Junk/Spam folder visible; move to/from junk (ORG-5).
- **S5.5** Create / rename / delete folders (ORG-6).
- **S5.6** Multi-select + bulk actions (ORG-7).

## M6 — Search
**Delivers:** SEARCH-1, SEARCH-2, SEARCH-3, OFF-2.
**Outcome:** fast search that works offline.

- **S6.1** Index synced messages into `tantivy`.
- **S6.2** Incremental indexing as mail arrives / changes.
- **S6.3** Search UI + query over sender / subject / body (SEARCH-1), near-instant (SEARCH-3).
- **S6.4** Verified offline against the local index (SEARCH-2, OFF-2).

## M7 — Multi-account + OAuth + onboarding
**Delivers:** ACC-1, ACC-2, ACC-5, ACC-6, ACC-8, MULTI-1, MULTI-2, APP-1.
**Outcome:** the effortless-setup hook is real — add Gmail/Outlook in a click, switch accounts.

- **S7.1** Gmail OAuth (loopback redirect; tokens in keychain) (ACC-1).
- **S7.2** Microsoft OAuth (gated on S0.6 approval) (ACC-2).
- **S7.3** Token refresh + re-authentication without data loss (ACC-8).
- **S7.4** Multiple accounts in store; sync scheduler handles N accounts (ACC-5); edit/remove (ACC-6).
- **S7.5** Per-account switcher UI (MULTI-1); correct from-address on reply (MULTI-2).
- **S7.6** One-click add-account onboarding flow (APP-1).

## M8 — Release
**Delivers:** READ-9, APP-2, APP-3, APP-4, APP-5, APP-6, PRIV-5†.
**Outcome:** the first releasable GeleitMail.

- **S8.1** Keyboard navigation + shortcuts (READ-9, APP-6).
- **S8.2** Light/dark theme (APP-3); settings / preferences (APP-4).
- **S8.3** Calm/fast final pass (APP-2): RAM, startup, large-mailbox performance.
- **S8.4** Cross-platform builds + installers — Windows, macOS, Linux (APP-5).
- **S8.5** Security review of crypto / OAuth / HTML paths; confirm no telemetry (PRIV-5).
- **S8.6** First-run polish, error/edge states; beta with real accounts; tag the first release.

---

## Beyond the first release (toward the full vision)
Slices defined when we reach them.

- **Unified inbox** (MULTI-3) — merged cross-account view.
- **Offline compose + reconciliation** (SEND-10/OFF-3, OFF-4).
- **Power search** (SEARCH-4 operators, SEARCH-5 cross-account).
- **Rules / filters** (ORG-8), **snooze** (ORG-9).
- **Notifications** (NOTIF-1, NOTIF-2, NOTIF-3).
- **Export / backup** (SEC-4), **self-update** (APP-7).
- **Later platforms** — mobile, then maybe web.

---

## Story coverage
Every story in `stories.md` maps to exactly one milestone above (or to "Beyond"). The only
milestone with no stories is M0 (infrastructure). When a story moves, update both files.
