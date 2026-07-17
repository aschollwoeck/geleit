# Changelog

## Unreleased

### Defer it for later
- **Snooze a message.** Can't deal with something yet? **Snooze** it — from an open message or a bulk
  selection — and pick when it comes back: later today, this evening, tomorrow, this weekend, next week.
  It leaves your inbox (and stops counting toward the unread badge) until then, when it reappears and
  notifies you as if it had just arrived. A **Snoozed** view in the rail shows everything waiting to come
  back, with an **Un-snooze** to pull one back now. Snooze is local to this device, and the times are in
  your own timezone.

### Always there
- **A system-tray icon** keeps GeleitMail running in the background. Closing the window now tucks it into
  the tray instead of quitting — so mail keeps arriving, and the count keeps updating. Click the tray
  icon and choose **Show GeleitMail** to bring the window back, or **Quit** to actually exit. Hovering it
  shows the same unread count as the title (*"GeleitMail — 3 unread"*). *(Linux needs the
  `libayatana-appindicator3` system library — see the README build prerequisites; and on a desktop with
  no tray, such as GNOME without an AppIndicator extension, closing quits as before so the window is
  never lost.)*

### New mail, instantly
- **New mail now arrives within seconds** on providers that support it (IMAP IDLE), instead of waiting
  for the periodic check. GeleitMail keeps a live connection open and the server tells it the moment
  mail lands; providers without it fall back to checking every few minutes, as before.


### Sending
- **Sending works offline.** Write a message with no connection and hit Send: instead of failing, it's
  kept in an outbox and goes out the next time GeleitMail reaches your provider. A quiet line under
  Compose shows how many are waiting — click it to open the **Outbox**, where you can **Retry** or
  **Discard** a message. If your provider *rejects* one (a bad address, say), it's shown with the reason
  rather than retried forever.
- **Fix a rejected message and resend it.** A send your provider turned down now has an **Edit** button
  in the Outbox: it reopens the message in Compose — recipients, subject, body, and attachments all
  intact — so you can correct the address (or whatever it was) and send again, instead of discarding and
  retyping. The original stays in the Outbox until the edited version goes out, so nothing is lost if you
  change your mind.

### Mail tells you it's here
- **Marking mail read or starred works offline.** A read or star you make with no connection is now
  remembered and sent to your provider the next time GeleitMail syncs, rather than being lost if the
  first attempt didn't get through.
- **Read your mail elsewhere and GeleitMail keeps up.** A message you read (or star) on your phone or in
  webmail stops being unread (or gains its star) here on the next check, and the unread count falls to
  match — so it reflects what you actually haven't read *anywhere*, not just on this device.
- **The window title shows your unread count** — *"GeleitMail — 3 unread"* — so a glance at the titlebar
  or taskbar says whether anything's waiting. It counts unread in your Inbox across every account, and
  reads just "GeleitMail" when you're all caught up.
- **New-mail notifications.** GeleitMail now shows a quiet desktop notification when mail arrives —
  the sender and the subject. The **Settings → Notifications** toggle finally does something: until now
  it saved a setting that nothing read.
- **Several messages at once are one notification** ("3 new messages — From Alice, Bob, Cara"), not a
  stack of popups. **Quiet hours** keep it silent overnight and tell you once in the morning. With more
  than one account, you can choose which ones you want to hear about.
- Mail you've already read elsewhere is never announced, and **mail that arrived while GeleitMail was
  catching up on older messages is no longer silently missed** — being told is now remembered per
  message, rather than being a property of whichever sync happened to fetch it first.

### Drafts, in one place
- **One Drafts.** The folder list showed "Drafts" twice — GeleitMail's own drafts and your provider's
  Drafts folder. Now there is one, and it holds both: the drafts you saved here *and* the ones you
  started in webmail or on your phone (marked **On your provider**). Continue one and it opens in the
  compose window, attachments and all; saving or sending it moves it here and removes their copy.
- **Fixed a way drafts could be destroyed.** A draft's identity on the server was derived from its row
  id — and those ids get reused. A new draft could inherit a deleted one's identity and, on its next
  save, wipe that draft's content off your provider. Drafts now keep an identity of their own.

### Your provider's folders, in your language
- **GeleitMail now asks your provider which folder is which** (IMAP SPECIAL-USE) instead of matching the
  English words. If your mail is in German, French, or anything else — *Entwürfe*, *Gesendet*,
  *Papierkorb* — those folders now get the right icons, sit in the right place, are protected from being
  renamed by accident, and are the folders GeleitMail actually uses. Before this, on such a provider,
  **the copy of every message you sent was saved nowhere**, and Archive, Delete and Empty Trash didn't
  work.
- **Move… moves the message to the folder you picked.** It used to sort every folder into one of four
  kinds and, for anything it didn't recognise — every ordinary folder — put the message in the Inbox.

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
