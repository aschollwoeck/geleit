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

## M0 — Foundations & feasibility — ✅ complete
**Outcome:** commit to the native UI stack *with evidence*, on a working scaffold — or pivot
before building on sand. *(Infrastructure — delivers no user stories directly.)*

- **S0.1** Cargo workspace scaffold; UI-agnostic engine crates vs. a UI crate; CI with
  `fmt --check` + `clippy -D warnings` + tests + `cargo mutants` wired.
- **S0.2** Spike: render a real-world HTML email in a *sandboxed* component, remote content blocked.
- **S0.3** Spike: virtualized message list rendering ~50k synthetic rows at 60fps in Slint.
- **S0.4** ADR: commit to Slint, or pivot — based on S0.2/S0.3 evidence.
- **S0.5** Platform-abstraction seams for keychain / HTML render / OAuth; Linux as primary dev OS.
- **S0.6** *(parallel admin track)* begin Google + Microsoft OAuth app-registration paperwork.

## M1 — Thin slice: read one account — ✅ complete
**Delivers:** ACC-3*, ACC-4*, SYNC-1*, SYNC-2, READ-1, READ-2, READ-3, READ-6, READ-7, SEC-2*.
**Outcome:** open the app, connect one IMAP account, see your folders and message list, read a
message in plaintext, and refresh — the whole stack proven end-to-end, live-verified against Dovecot.

> **Carried forward (the `*` items):** S1.10 added an in-app **Add-account** form (manual IMAP), so
> account setup no longer needs env vars; OAuth + provider auto-config remain **M7**. The keychain is
> still the **seam + in-memory double** — credentials don't persist across restarts (SEC-2 backend →
> M2). Read-state is **local only** (server write-back → M6); the folder list doesn't live-update
> after refresh (→ next launch / M2). Deliberate, documented deferrals — not gaps in what shipped.

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
- **S1.10** Add-account screen (manual IMAP config) — completes the in-app side of ACC-3: create an
  account from the UI, persist its IMAP settings, first sync. *(Credential persistence still M2.)*

## M2 — Robust engine & store
**Delivers:** SEC-2, SEC-1, SEC-3, SYNC-3, SYNC-4, SYNC-1†, OFF-1.
**Outcome:** correct, robust, encrypted local sync — now that we can see what we're building.

Re-planned at the M1→M2 boundary: the **real OS keychain** moved first (it persists credentials —
the S1.10 gap — and holds the at-rest key the next slice needs).

- **S2.1** ✅ Real OS keychain backend (SEC-2) — `OsSecretStore` over the Secret Service; the app
  uses it, so passwords persist across restarts. *(macOS/Windows stores enabled at M8 packaging.)*
- **S2.2** ✅ Encryption at rest (SEC-1) — SQLCipher, key in the keychain, transparent unlock (ADR-0008).
- **S2.3** ✅ Incremental sync: detect new / deleted, UIDVALIDITY-safe (SYNC-1 robust). *(Server→local
  flag-change sync deferred to M6 with write-back, to avoid clobbering local read-state.)*
- **S2.4** ✅ Progressive backfill of the full mailbox, newest-first, batched, in background (SYNC-3).
- **S2.5** Gmail-specific handling (labels-as-folders, X-GM-EXT-1). *(Needs a real Gmail account to verify.)*
- **S2.6** ✅ Non-blocking sync status / progress (SYNC-4) — calm progress strip, distinct from errors.
- **S2.7** ✅ Sync integrity: idempotent, resumable, provably no dupes/loss (proptest).
- **S2.8** ✅ Offline reading verified (OFF-1); wipe local data on account removal (SEC-3).
- **(follow-up)** `zeroize` secret + key buffers where practical (§9; ADR-0004/0008).

## M3 — Rich, safe reading ✅ COMPLETE
**Delivers:** READ-4, READ-5, READ-8 (view), PRIV-1, PRIV-2, PRIV-3, PRIV-4.
**Outcome:** read real HTML mail safely, in threads, with attachments.

- **S3.4** ✅ Conversation threading — detect conversations + count (READ-5). *(Full thread view: follow-up.)*
- **S3.5** ✅ Attachments: **view** name/type/size (READ-8 view half). *(Save-to-disk: follow-up.)*
- **S3.1** ✅ Sandboxed HTML renderer embedded in the reading pane (READ-4) + sanitization
  (PRIV-1 remote blocked, PRIV-4 no scripts).
- **S3.2** ✅ Hardening: CSP belt-and-suspenders (`default-src 'none'`) + sandbox-escape tests.
- **S3.3** ✅ Per-message "load remote content" opt-in (PRIV-2) + "remote content blocked" cue (PRIV-3).

> The webview (wry-in-Slint, `build_as_child`) is **X11 only**; on Wayland the reading pane falls back
> to the plain-text view (graceful). The **security** (sanitization, no-script, no-remote, CSP) is
> machine-verified; the **visual fidelity** of rendered mail needs the maintainer's eyes on a running
> window — the one place "build + self-verify" needs the maintainer.
> **M3 follow-ups:** full thread-navigation view; save-attachments-to-disk; trusted-sender persistence
> (always-load); CSS-aware sanitization (fidelity); Wayland embedding.

## M4 — Send ✅ COMPLETE
**Delivers:** SEND-1…SEND-9, ACC-7.
**Outcome:** full compose / reply / reply-all / forward for one account.

- **S4.1** ✅ SMTP transport (`lettre` + rustls, ADR-0009); in-process sink test (CI).
- **S4.2** ✅ Message building (`mail-builder`) + compose window — new message (SEND-1).
- **S4.3/S4.5/S4.9** ✅ Reply / reply-all / forward with quoting + threading (SEND-2, SEND-3).
- **S4.4/S4.11/S4.13** ✅ Attachments in compose + native file picker (zenity/kdialog) (SEND-4).
- **S4.5/S4.10** ✅ Drafts: save and resume (SEND-5).
- **S4.6** ✅ Basic formatting via **Markdown** → multipart/alternative (SEND-6).
- **S4.7** ✅ Per-account signature, auto-included (SEND-7, ACC-7).
- **S4.8** ✅ Save sent mail to Sent via IMAP APPEND (SEND-8).
- **S4.12** ✅ Address autocomplete from history — To field (SEND-9).

> **Follow-ups (backlog):** outbox + retry / offline-send (SEND-10); SPECIAL-USE Sent detection;
> persist attachments in drafts; Cc autocomplete; in-process file-picker (rfd/portal). The webview
> uses Slint's **software renderer** to coexist with webkit's GL (X11; PR #53). Sending verified
> end-to-end by the in-process SMTP sink; live IMAP APPEND has an `#[ignore]` test.

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
