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
- **App** — `export_folder(account_id, folder_id)`: on a worker, gather each message's header + body,
  build its `.eml` (`message::export_eml`) and mbox record, concatenate, and write to a path from the
  native save dialog (default name `<FolderName>.mbox`). Returns `false` if the user cancels.
- **UI** — an **Export** button in the list header, shown only for a real folder view (not
  drafts/outbox/snoozed, not the merged "All inboxes"). A toast confirms *"Exported 42 messages"*.

## Fidelity — named honestly

The `.eml` is **reconstructed from what we store** — headers + the text/HTML bodies — exactly as
"Save as .eml" already is. So the export is a faithful copy of every message's **text**, but **attachment
bytes are not included**: this client stores attachment *metadata*, fetching the bytes from the provider
on demand, so they aren't ours to write into the file. (A single attachment can still be saved from its
message; a full-fidelity, attachment-included export means fetching every message's raw bytes from the
server and is a named follow-up.) The mbox `From ` date is the message's own date.

## Out of scope (named)

Account-wide "export everything" in one action (per-folder composes to the same end; a one-click
whole-account export is a follow-up); attachment bytes (see above); importing mbox *in*; other formats
(Maildir, PST); scheduled/automatic backups.

## Acceptance criteria

1. `fmt` / `clippy -D warnings` / test / `cargo deny check` / boundary all green; `mbox_entry` mutants
   0-missed; `perf-budget` unaffected.
2. `mbox_entry` frames a record correctly and escapes `From `-lines reversibly — unit-tested, including a
   body line that begins with `From ` and one that begins with `>From `.
3. A folder exports to a well-formed mbox that re-parses (each record's `.eml` parses back to the right
   subject/body) — engine/store tested; snoozed messages are included.
4. The Export button writes the file end to end — the maintainer's eyeball on the running app.
