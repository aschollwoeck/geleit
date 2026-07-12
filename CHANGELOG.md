# Changelog

## v0.1.1 — Tauri + Leptos rebuild & the "Soft daylight" design — 2026-07-12

The reading pane *is* the product, and no Rust-native path rendered real mail correctly — so the UI
was rebuilt on **Tauri (OS webview) + Leptos** (Rust→WASM), replacing Slint (ADR-0012). HTML mail now
renders faithfully in a sandboxed `mail://` iframe (no scripts, no same-origin, CSP) with **zero**
of the old rendering workarounds. Still Rust end to end, still no HTTP client, still no telemetry.

### Design
- A complete **"Soft daylight"** visual overhaul: quiet, rounded, roomy; a 3px accent guide edge on
  whatever has attention; deep-indigo primary action; Hanken Grotesk + IBM Plex Mono bundled locally
  (never fetched at runtime). Light + dark throughout.

### Reading & organizing
- **Undo** for archive / delete / spam — the move is deferred through the toast window, so Undo is a
  pure local cancel that can't lose mail.
- **Keyboard navigation:** `j`/`k` (or ↑/↓) move through the list; `z` undoes.
- "Mark as read when opened" (General settings) is now actually honored.
- Reading-pane action buttons are pinned above the sender and subject, so they don't shift as the
  subject wraps.

### Accounts
- A merged **"All inboxes"** view — every account's inbox in one date-sorted list, each row tagged
  with its account; search and refresh span all accounts.
- Account switcher, add-account wizard, and remove-account in the new UI.

### Composing
- Recipient **chips** for To/Cc (de-duplicated), a Discard button, and file **attachments** via the
  native picker.

### Notes
- First release built on the new stack; sign-in is still manual IMAP/SMTP (one-click OAuth is planned).
- Linux only. The release tarball now builds the WASM frontend before packaging.

## v0.1.0 — first release (Linux) — 2026-06-22

GeleitMail's first releasable build: a native, local-first, privacy-first email client in Rust
(Slint UI). No middleman, no telemetry, no tracking.

### Accounts & sync
- Add accounts with manual IMAP/SMTP settings; **multiple accounts** with a rail switcher and correct
  from-address per account (MULTI-1/2, ACC-5/6).
- Incremental sync + progressive backfill; offline reading from the local store.
- Credentials in the OS keychain; **encrypted database at rest** (SQLCipher; SEC-1).

### Reading
- Folder list + virtualized message list + conversation grouping.
- **Sandboxed HTML rendering** (webview behind CSP `default-src 'none'`, JS disabled) with
  remote-images **opt-in** per message (PRIV-2/3). Attachments listed.

### Sending
- Compose / reply / reply-all / forward with correct quoting + threading; per-account signature.
- Attachments (native file picker) **persisted in drafts**; save/resume drafts; save to Sent.
- Basic formatting via Markdown → multipart/alternative. Address autocomplete for To **and** Cc.

### Organizing
- Star, archive, delete→trash, move, empty-trash / permanent-delete, junk in/out — optimistic with
  server write-back (no dupes/loss). Create/rename/delete folders. Multi-select + bulk actions.

### Search
- Instant offline full-text search (SQLite FTS5, in the encrypted DB) over sender/subject/body, with
  `from:` / `subject:` / `has:attachment` operators and match-context snippets.

### App
- **Keyboard navigation** (j/k, c, r, Esc); **light/dark theme** + settings (persisted); calm/fast
  release build (~26 MB). Security & privacy review with an **enforced no-telemetry** dependency ban.

### Known limitations
- Linux only (X11 for the HTML viewer; Wayland falls back to text). macOS/Windows need webview +
  keychain porting.
- OAuth (Gmail/Outlook one-click) not yet available — manual IMAP/SMTP for now.
- Read/flag state is local (no server `\Seen`/`\Flagged` write-back yet); sync is on-demand per
  visible account.
