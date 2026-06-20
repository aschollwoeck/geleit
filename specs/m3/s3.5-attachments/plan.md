# S3.5 — Attachments (view) · Plan (the HOW)

Implements `spec.md`.

## geleit-engine::mime (pure, mutation-tested)
- `struct Attachment { filename: Option<String>, content_type: String, size: u64 }`.
- `ParsedBody` gains `attachments: Vec<Attachment>`.
- In `parse_body`, iterate `msg.attachments()` (mail-parser `MimeHeaders`): `attachment_name()` →
  filename; `content_type()` → `"{ctype}/{subtype}"` (subtype optional); `len()` → size.
- Tests: the multipart fixture (has `note.txt`) → one attachment with name/type/size; plain message → none.

## geleit-store
- Migration **#4**: `CREATE TABLE attachment (id INTEGER PRIMARY KEY, message_id INTEGER NOT NULL
  REFERENCES message(id) ON DELETE CASCADE, filename TEXT, content_type TEXT NOT NULL, size_bytes
  INTEGER NOT NULL); CREATE INDEX attachment_message ON attachment(message_id);`
- `struct Attachment { filename: Option<String>, content_type: String, size: i64 }` (store-side).
- `store_attachments(message_id, &[Attachment])`: delete existing rows for the message, insert the
  given (atomic) — idempotent re-sync.
- `attachments_for(message_id) -> Vec<Attachment>`.
- Tests: round-trip, replace-on-re-store, cascade on message/account delete.

## sync
- In `imap::fetch_bodies_for`, after `parse_body` + `store_body`, map `parsed.attachments` →
  `store::Attachment` and `store.store_attachments(message_id, …)`.

## geleit-app
- `viewmodel::attachment_label(filename: Option<&str>, size: u64) -> String`: `"{name} · {human}"`
  (human size: B / KB / MB, 1 decimal); fallback name "(unnamed)". Pure → tested.
- Reading pane: an `attachments` model (Slint `[string]` labels) set on `message-selected` from
  `store.attachments_for(id)`; a small section under the body shown when non-empty.

## Verify
gates; mime/store/viewmodel tests; live (synced message with attachment → `attachments_for`
non-empty); app launches; `cargo mutants`. `.cargo/mutants.toml` unchanged (new logic is in
mutation-tested files).
