# Changelog

## v0.1.2 — feature parity restored, and then some — 2026-07-13

The v0.1.1 rebuild got the rendering and the design right, but it had quietly left several v0.1.0
features behind. They are all back — and the three that had shipped half-finished are now finished.
Everything below is local-first and private by default, as ever: no telemetry, no HTTP client, and
your mail encrypted on disk.

### Reading
- **Attachments** are listed on an open message (name + size), and you can **save one to disk** — it's
  fetched from your provider on demand, since attachment bytes aren't kept locally.
- **Save a message as `.eml`**, and **open a `.eml` file** from disk to read it (it lands in a local
  *Saved* folder). Handy for keeping — or sending someone — a copy of a message.

### Organizing
- **Star** a message to find it again; starred rows show a ★.
- **Empty Trash** and **Delete forever** — both irreversible, so both ask first.
- **Folders you make yourself:** create, rename, and delete them. Renaming keeps the messages;
  deleting a folder deletes the mail in it (after a confirmation). Your standard folders are left
  alone.
- **Act on several messages at once:** hover a row for a checkbox, **Shift-click** to take a whole
  run, then Archive, Delete, or Mark read / unread the lot — with a single **Undo**.
- **Esc** closes the search box.

### Writing
- **Drafts**: save a half-written message and pick it up later — **attachments included**. Sending or
  discarding a draft clears it.
- **Markdown**: turn it on in the compose footer to send *bold*, lists, links and tables. A plain-text
  version always goes along, so nobody gets a wall of markup.
- **Address suggestions** as you type a recipient, drawn from people you've had mail from.

### Privacy
- **Drafts stay on this device by default**, encrypted like the rest of your mail — nothing you
  haven't sent is uploaded. If you *want* them on your phone or webmail, the new
  **Settings → Privacy → "Sync drafts to your provider"** keeps them in your provider's Drafts folder
  too. It's **off** unless you turn it on, and turning it back off takes those copies off the server
  again.

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
