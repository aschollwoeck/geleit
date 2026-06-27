# Slice: Save & Open mail files (.eml)  — READ-10

## Goal
Let the user **save** the open message to a `.eml` file on disk, and **open** a `.eml` file back
into the app to read it. A normal, always-available feature — not a dev tool. Primary uses: archive
a message to disk, move a message between machines, and share a message (e.g. to reproduce a
rendering issue) as a standard, portable file any mail client understands.

## Behaviour (default)
- **Save:** the reading-pane action row gains **"Save"**. Clicking it opens the desktop's native
  *save* dialog (pre-filled with a filename derived from the subject), and writes the open message as
  an RFC 822 `.eml` file (subject, From/To/Cc, Date, Message-ID, and the text + HTML bodies).
- **Open:** the left rail gains **"Open mail file…"**. Clicking it opens the native *open* dialog;
  the chosen `.eml` is parsed and stored in a local **"Saved"** folder on the current account, then
  selected so it renders in the reading pane exactly like any other message (HTML via the CPU
  renderer, links, attachments-list, etc.).
- The **"Saved"** folder is local-only: it is **never** removed by folder pruning on refresh and is
  never written to the IMAP server. Imported messages have no server UID.
- Opening a file when there is no account yet shows a short status asking to add an account first.

## Non-goals (this slice)
- Round-tripping byte-exact original MIME (we rebuild the `.eml` from stored parts; the bodies are
  faithful, headers are reconstructed). Saving received attachments' bytes (not stored yet).
  Drag-and-drop. `.mbox`/multi-message files.

## Design
- **Engine** (`geleit-engine::message`):
  - `export_eml(header, body) -> Result<Vec<u8>, String>` — build RFC822 via `mail-builder` from a
    stored `MessageHeader` + `StoredBody` (From/To/Cc/Subject/Date/Message-ID + text/html parts).
  - `ImportedEml` + `parse_eml(raw: &[u8]) -> ImportedEml` — parse via `mail-parser` (decodes RFC2047
    headers) into subject/from/to/date/message-id + plain/html (body via the existing `mime::parse_body`).
- **Store**: `prune_folders` always keeps local folders (the `"Saved"` name) so import survives refresh.
- **App**: generalise the native picker to support a **save** mode + default filename; `save-message`
  and `open-mail-file` callbacks; import upserts the Saved folder + message + body, then reloads.

## Acceptance criteria
1. Saving the open message writes a valid `.eml` that re-opens in GeleitMail (and other clients).
2. Opening that `.eml` shows the same subject/sender and renders the same body (HTML included).
3. `export_eml` → `parse_eml` round-trips subject + from + plain + html (engine unit test).
4. The "Saved" folder survives a folder prune (store unit test).
5. build / test / `clippy -D warnings` (+`dangerous-tls`) / `fmt` / `cargo deny` green; pure logic
   mutation-clean.
