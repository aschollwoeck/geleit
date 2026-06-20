# GeleitMail — Project Handoff

A privacy-focused, local-first email client. Alternative to Outlook, eM Client, and Mailbird.

This document captures decisions made so far so any Claude session (including Claude Code) can pick up the thread.

---

## Positioning

**Core angle:** Privacy & local-first. Mail, indexes, and keys live on the device. No telemetry, no cloud middleman, no tracking-pixel leakage.

**Who we're beating, and how:**
- **Outlook** — cloud-tied, telemetry-heavy. We win on privacy and respecting the user's machine.
- **eM Client** — polished native client, but Windows-centric and not privacy-positioned. We match polish, beat on privacy + true cross-platform.
- **Mailbird** — Electron-based, RAM-heavy. We win on being lean and local-first.

---

## Scope

**Decision: desktop-first.** Do NOT attempt Windows + cross-platform desktop + web + mobile simultaneously — that's four products with conflicting architectures and the usual way projects like this die.

- **Phase 1:** Cross-platform desktop (Windows + macOS + Linux) from one codebase. Genuinely local-first.
- **Later (only if traction):** mobile, then maybe web.
- **Web is in tension with local-first** by design (browser storage limits, data passing through infra) — deliberately deferred.

---

## Tech Stack

### Backend (the hard 80% — identical regardless of UI choice)

Pure Rust. This is where the real work and real risk live:

- **IMAP sync engine** — `async-imap`. A correct, fast sync engine is the make-or-break component; this is where Thunderbird forks usually stall.
- **SMTP send** — `lettre`.
- **MIME parse/build** — `mail-parser` / `mail-builder`. Watch encoding edge cases.
- **Local full-text search** — `tantivy`.
- **Local store** — `rusqlite` (SQLite). Encrypted at rest.
- **OAuth** — Gmail and Microsoft OAuth are mandatory now. Getting Microsoft to approve a new client's OAuth app is real friction — start that process early.
- **HTML email rendering** — must be safe: sandboxed, no remote content by default (blocks tracking pixels). Security-critical.

### UI (swappable front layer — the easier part, decision still OPEN)

Focus is Rust-native. Leading candidates:

- **Tauri** — web frontend (React/Svelte/Solid) in OS-native webview. Mature, fastest path to a good-looking dense UI, tiny binaries vs Electron. Downside: per-OS webview rendering differences (WebView2 / WebKit). **Current lean.**
- **Slint** — truly native Rust UI, no webview, lean. Best "pure Rust" option. Downside: younger ecosystem, fewer complex prebuilt widgets (rich text, advanced lists) — more built by hand.
- **Iced** — pure Rust, retained-mode, Elm architecture (powers System76 COSMIC). Credible no-web option but pre-1.0, missing widgets.
- **Dioxus** — React-like; still leans on webview for desktop, so not really escaping the Tauri model.

Rejected for this use case: **egui** (immediate-mode is awkward for text-heavy, accessibility-sensitive, stateful mail UI), **Electron** (undercuts the lean/privacy pitch).

**Key insight:** the backend (IMAP/crypto/SQLite/tantivy) is identical whether UI is Tauri or Slint. So the engine can be started now and the UI-framework decision deferred.

---

## Build Order (recommended)

1. Backend engine first: IMAP sync + local SQLite store + MIME parsing. Get reliable sync working against a real Gmail/IMAP account.
2. Add `tantivy` local search over the synced store.
3. OAuth (Gmail + Microsoft) — start Microsoft app approval early due to lead time.
4. Safe HTML rendering (sandboxed, remote content blocked by default).
5. SMTP send path.
6. Only then: commit to UI framework (Tauri vs Slint) and build the dense client UI on top of the stable engine.

---

## Open Decisions

- [ ] UI framework: Tauri vs Slint (lean Tauri for speed-to-product; Slint for pure-native/leanest).
- [ ] Encryption-at-rest approach for the local SQLite store + key management.
- [ ] Licensing / open vs closed source (affects e.g. Qt-style concerns, Slint licensing tier).
- [ ] Branding: name "GeleitMail" — no existing email client found by that name; verify USPTO/EUIPO + domain (geleitmail.com / .app) before committing.

---

## Naming note

"Geleit" (German: escort / safe passage / safe conduct) is the distinctive lead element; "-Mail" is the descriptive suffix. The metaphor fits the privacy/local-first angle well — mail under safe escort, kept on the user's own machine. The "-Mail" suffix space is crowded (Canary, Proton, Zoho, GMX, K-9...), but "Geleit" stands cleanly on its own. Confirm with a proper USPTO/EUIPO (and DPMA, given the German market) trademark search and a registrar check (geleitmail.com / .app / .de) before committing.
