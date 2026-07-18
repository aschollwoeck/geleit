# Export mail to mbox (SEC-4)

**Constitution:** P2 (privacy — your data is yours), P1 (the UI never waits), P8 (spec-driven).
**Story:** SEC-4 — export / back up my mail.

## Why

A local-first, privacy-first client owes you an exit. If your mail lives on your device, you must be able
to *get it out* — into a portable, app-independent file you can archive, grep, or import into any other
client. **mbox** is that format: a single plain file, one concatenated message after another, read by
Thunderbird, mutt, `grep`, Python's `mailbox`, everything.

## The shape

Export the **currently-viewed folder** to an `.mbox` file the user picks. Reuses the same `.eml`
reconstruction that "Save as .eml" (READ) already builds from a stored message, wrapped in mbox framing.

- **Store** — `folder_message_ids(folder_id)`: every message in the folder, oldest-first (an archive
  reads chronologically), **including snoozed ones** (a backup is complete; snooze is a view state, not a
  deletion).
- **Engine** — `mbox_entry(sender, when, eml)`: one mbox record. The `From ` separator line
  (`From <sender> <asctime>`), then the message with any `From `-at-line-start escaped mboxrd-style
  (`From ` → `>From `, `>From ` → `>>From `, reversibly), then a blank line. Pure — unit- and
  mutation-tested.
- **App** — `export_folder(folder_id, folder_name)`: on a worker, pull the folder's raw originals from
  the server (`folder_mbox_complete` → `run_fetch_folder_raws`, best-effort), then frame each message —
  the raw original when fetched, else the reconstructed `.eml` (`message::export_eml`) — into the mbox
  via the testable seam `build_folder_mbox`, and write to a path from the native save dialog (default
  name `<FolderName>.mbox`). Returns `None` if the user cancels, `Some(0)` for an empty folder (no
  dialog), `Some(n)` after writing `n`. A single message that can't be read or rebuilt is skipped, not
  fatal. `export_account` does the same folder-by-folder (fetch → build → next), so memory stays bounded
  to one folder's raws at a time.
- **UI** — an **Export** button in the list header, shown only for a real folder view (not
  drafts/outbox/snoozed, not the merged "All inboxes"). A toast confirms *"Exported 42 messages"*.

## Fidelity — complete when online, still a backup offline

The export now writes each message's **true raw original**, pulled from the server (`BODY.PEEK[]`, one
session per folder via `imap::fetch_raw_batch` → `sync_actions::run_fetch_folder_raws`) — so
**attachments are included**, byte-for-byte as the provider holds them. The store only ever keeps
attachment *metadata* (bytes are fetched on demand), so a faithful backup has to fetch; this is that
follow-up, shipped.

It stays **best-effort**, so an export never fails for want of a connection: a message whose raw can't be
fetched (offline, a local-only Saved message with no uid, or a uid the server no longer has) falls back
to the old **reconstruction from the stored envelope + body**. So an export is *complete* when online and
*still a text backup* offline — never nothing. `build_folder_mbox` takes a `uid → raw` map and prefers
the raw (moving each out of the map as it's written, so peak memory is one folder's mbox, not that plus a
second copy of every raw); `mbox_entry` frames whichever bytes it's given. The mbox `From ` date is the
message's own date.

**Bounded, not hangy.** Three guards keep the fetch from becoming a footgun:
- The `UID FETCH` is issued in **chunks** (`RAW_FETCH_CHUNK = 256`), never one command carrying a whole
  folder's uids — a hundreds-of-KB command line a server or proxy could reject, which would silently drop
  the folder to reconstruction.
- The **connect** (TCP + TLS + login) is capped at `CONNECT_TIMEOUT_SECS = 15`, so an unreachable server
  degrades in seconds rather than hanging on the OS TCP timeout (~75–120s). The data fetch is left
  unbounded — a big folder legitimately takes a while, and cutting it off would turn a slow-but-complete
  export into an incomplete one.
- `export_account` **fails fast**: `run_fetch_folder_raws` returns `None` when it couldn't reach the
  server, and the loop then skips the network for every remaining folder — so an offline whole-account
  export costs one connect attempt, not one per folder. A store error on a single folder skips just that
  folder, never the whole account.

## Out of scope (named)

Importing mbox *in*; other formats (Maildir, PST); scheduled/automatic backups. *(Whole-account export —
`export_account`, Settings → Accounts → **Export mail**, one `.mbox` per folder into a chosen directory —
and attachment-included export were follow-ups, now both shipped.)*

## Known limitations (named honestly)

- **The count doesn't distinguish complete from degraded.** The toast says *"Exported N messages"* — true
  in number, but it doesn't say how many fell back to text-only (a partial server failure, or offline).
  Surfacing *"N exported, M text-only"* is a follow-up; the export is never silently *empty*, only
  possibly text-only for some messages.
- **No UIDVALIDITY recheck.** The raw fetch trusts the stored uids; if the server reset UIDVALIDITY since
  the last sync, a uid could name a different message. Rare, and shared with the existing single-attachment
  save path (`fetch_raw_message`); a UIDVALIDITY guard across both is a separate follow-up. Streaming the
  mbox to disk (rather than building it in memory) also remains a named follow-up.

## Acceptance criteria

1. `fmt` / `clippy -D warnings` / test / `cargo deny check` / boundary all green; `mbox_entry` mutants
   0-missed; `perf-budget` unaffected.
2. `mbox_entry` frames a record correctly and escapes `From `-lines reversibly — unit-tested, including a
   body line that begins with `From ` and one that begins with `>From `.
3. A folder exports to a well-formed mbox that re-parses (each record's `.eml` parses back to the right
   subject/body) — engine/store tested; snoozed messages are included.
4. When a message's raw is supplied, the export writes it **verbatim** (attachments included) rather than
   the reconstruction; with no raw it falls back — unit-tested (`build_folder_mbox_prefers_...`). Live
   against Dovecot: an appended message with an attachment is fetched and its attachment appears in the
   export (`examples/live_export_attachment.rs`).
5. The Export button writes the file end to end — the maintainer's eyeball on the running app.
