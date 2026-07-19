# Changelog

## Unreleased

### GeleitMail in your browser (self-hosted)
- **You can now run GeleitMail as a local web app, alongside the desktop version.** Start the bundled
  server on your own machine and open GeleitMail in any browser — same interface, same mail, same
  encrypted store. It listens only on your own computer (`127.0.0.1`), so nothing else on the network
  can reach it and your mail never leaves your hardware. Run `cargo run -p geleit-server` (dev) and
  visit `http://127.0.0.1:8080`. This is the groundwork for GeleitMail on more platforms; the desktop
  app is unchanged.
  - *Note:* run the desktop app **or** the web server against a given mailbox, not both at once.
  - **Saving and attaching files works the browser way in the web version.** Save a message or an
    attachment and it downloads through your browser; attach files to a message and they upload from the
    browser's file picker — no dialogs popping up on the server. **Exporting a folder** downloads its
    `.mbox` through the browser too. (Exporting a *whole account* — one file per folder — is still
    desktop-only for now; export folders one at a time in the web version.)
  - **The web version now keeps your mail up to date on its own** — the same background sync as the
    desktop app (periodic checks, instant IMAP push for new mail, and the gradual full-mailbox
    download) runs in the server, so new mail and notifications arrive without pressing Refresh.
  - **You can open the web version from another device on your network, safely.** Set a bind address
    and a password and GeleitMail becomes reachable from your phone or laptop with a login prompt;
    without a password it refuses to leave your own machine, so it's never exposed unprotected. Put
    it behind an HTTPS reverse proxy for the LAN (see the README).

### Move several messages to a folder at once
- **The multi-select bar now has a "Move to folder" button.** Select a batch of messages and move them
  all into any folder in one go, with the same Undo window as bulk Archive and Delete.

### Under the hood: tighter secret handling
- **Your encryption key and passwords are wiped from memory as soon as they're used**, instead of
  lingering in freed memory, and the whole app got a fresh security review (no issues found in its
  no-tracking, encrypted-at-rest, and safe-mail-rendering guarantees).

### Changes from your other devices sync in everywhere
- **Read a message, star it, or delete it on your phone, and it updates in every folder — not just the
  inbox.** The background sync already reconciled the inbox with your other devices; now it does the same
  for all your folders, so archived and filed mail stays in step too.

### All your accounts stay complete in the background
- **Every account's full mailbox now downloads in the background, not just the one you're viewing.**
  GeleitMail progressively catches up all folders of all accounts, so a secondary account you rarely open
  still becomes fully searchable and available offline over time — gently, one folder at a time.

### New accounts get new mail instantly
- **An account you add starts receiving instant new-mail push right away.** Previously a newly-added
  account only checked for mail on the periodic background sync until you restarted; now it gets the same
  live push as your other accounts the moment it's added.

### Moves work on more mail servers
- **Archive, delete, and move now work on servers without the IMAP `MOVE` extension.** On a provider
  that doesn't support `MOVE`, GeleitMail now files mail the portable way (copy, then remove the original)
  instead of the action silently failing.

### Backups now keep your attachments
- **Exported mail includes attachments.** Exporting a folder or a whole account to `.mbox` now writes
  each message exactly as your server holds it — attachments and all — instead of text only. When you're
  offline it still exports the text of every message, so a backup is always complete when connected and
  never empty-handed when not. If some messages could only be saved as text, the confirmation tells you
  how many, so you always know whether a backup is complete.

### Organizing works offline
- **Archive, delete, and move mail without a connection.** Filing mail no longer needs a signal:
  Archive, Delete, mark as Spam, and Move to… now take effect instantly whether you're online or not.
  A move made offline is remembered and reaches your server the moment you reconnect — it won't snap
  back with an error, and your mail is never lost while it waits. (Permanently emptying the Trash still
  needs a connection.)

### Notifications you can click
- **Click a new-mail notification to jump to GeleitMail.** New-mail notifications are now clickable — clicking one brings the app to the front.

### Keeps itself current
- **GeleitMail can update itself.** It checks for a newer **signed** release and, when you choose,
  installs it and restarts — so security and bug fixes reach you without a manual re-download. In
  **Settings → General** you'll find your version, a **Check for updates** button, and an **Automatically
  check for updates** switch (on by default). The check contacts only the release server and sends
  **nothing about you or your mail** — it just fetches a public list of releases and compares versions on
  your device — and an update that isn't correctly signed is refused. Installing is always your call.

### Mail that sorts itself
- **Rules that auto-sort your inbox.** In **Settings → Rules**, set up *when this, do that*: when a
  message's **From**, **Subject**, or **To** contains some text, **move it to a folder**, **mark it
  read**, and/or **star it**. New mail is sorted as it arrives; rules run on your device when GeleitMail
  checks your mail, and the first rule a message matches wins — reorder them with **↑ ↓** to set which
  takes priority. Already have a full inbox? **Run on inbox now** applies your rules to what's already
  there.

### Your mail is yours
- **Export your mail to standard mbox files.** The **Export** button in the message-list header writes the
  folder you're viewing to a portable `.mbox` file — the format Thunderbird, mutt, `grep`, and every other
  mail tool can read — and **Settings → Accounts → Export mail** exports a whole account, one `.mbox` per
  folder, into a directory you choose (so the folder structure is kept). Archive or move your mail whenever
  you like. (It writes each message's text faithfully; attachment *files* aren't included, since GeleitMail
  keeps them on your provider and fetches them on demand — save an attachment individually if you need it.)

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
