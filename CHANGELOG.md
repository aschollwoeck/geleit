# Changelog

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
