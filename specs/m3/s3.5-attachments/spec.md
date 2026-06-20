# S3.5 — Attachments (view) · Spec (the WHAT)

First slice of **M3**, sequenced first because it's the most self-contained + fully offline-
verifiable. Type: engine + store + UI. Delivers the **view** half of **READ-8**: a person can see
what's attached to a message (name, type, size). Saving to disk is a follow-up.

Status: **draft.**

## Purpose
When a message has attachments, the reading pane lists them — filename, kind, human-readable size —
so you know what arrived, all from the local (encrypted) store, offline.

## In scope
- `geleit-engine::mime`: `parse_body` also extracts attachment metadata (filename, content-type,
  size) via `mail-parser` — pure, mutation-tested.
- `geleit-store`: an `attachment` table (migration #4); `store_attachments` (replace-per-message)
  + `attachments_for`; cascade on message/account delete.
- sync: when a body is fetched/parsed, store its attachment metadata too.
- `geleit-app`: `viewmodel::attachment_label` (name + human size) tested; the reading pane shows an
  attachments section for the selected message.

## Out of scope
- **Saving/opening** attachments (follow-up — needs the bytes stored or re-fetched + a file dialog).
  HTML rendering (later M3 slice). Threading (later M3 slice). Inline images.

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. `parse_body` extracts attachments from a multipart fixture (name/type/size) — tested; a no-
   attachment message yields none.
3. store: `store_attachments`/`attachments_for` round-trip; replace-per-message; cascade on delete — tested.
4. `attachment_label` formats name + human size (B/KB/MB) — tested.
5. **Live (`--features dangerous-tls`):** a synced message with an attachment has its attachment
   metadata in the store after sync.
6. `cargo mutants` — mime/store/viewmodel logic covered; imap.rs/refresh.rs/main.rs excluded; 0 missed.

## Deliverables
- `mime` attachment extraction; store `attachment` table + methods; sync wiring;
  `attachment_label`; reading-pane attachments UI; live test; manual touch.
