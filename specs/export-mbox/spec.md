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
- A **UIDVALIDITY guard**: `fetch_raw_batch` compares the server's current UIDVALIDITY to the value the
  folder was synced at; on a mismatch (the server reset its UIDs since sync) the stored uids now name
  *different* messages, so it fetches nothing and the build reconstructs — never splicing the wrong
  message's content into the archive.

## Honest by count

The export reports an [`ExportSummary`] (`{ exported, text_only }`): how many messages were written, and
how many went out **text-only** — reconstructed because their raw couldn't be fetched, so any attachments
aren't in the file. The toast (`view::export_message`) reads *"Exported 42 messages"* when every message
was complete, and *"Exported 42 messages · 3 text-only (attachments not saved)"* when some weren't — so a
degraded backup is never silently passed off as a full one.

## Streamed to disk

`export_account` chooses the destination directory **first**, then builds and writes each folder's
`.mbox` before starting the next — so peak memory is one folder's mbox, not every folder's at once (it
used to accumulate them all). Empty folders are skipped up front (`store::folder_message_count`), so an
empty account is known before any dialog and no empty file is written. (Per-message streaming *within* a
folder remains a follow-up — one folder's mbox is still built in memory.)

## Out of scope (named)

Importing mbox *in*; other formats (Maildir, PST); scheduled/automatic backups. *(Whole-account export —
`export_account`, Settings → Accounts → **Export mail**, one `.mbox` per folder into a chosen directory —
and attachment-included export were follow-ups, now both shipped.)*

## Known limitations (named honestly)

- **Per-message streaming within a folder** isn't done: one folder's mbox is still assembled in memory
  before it's written. The whole-account export no longer accumulates *every* folder (see "Streamed to
  disk"), so the bound is one folder, not the account — but a single enormous folder still builds in RAM.
- The **single-attachment save path** (`fetch_raw_message`, READ-8) does **not** yet share the export's
  UIDVALIDITY guard; a guard across both is a small follow-up.
- *(Resolved this slice: the text-only count is now surfaced, and the export applies a UIDVALIDITY guard —
  see above.)*

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
