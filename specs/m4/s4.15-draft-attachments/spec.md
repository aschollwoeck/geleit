# S4.15 — Attachments persisted in drafts (SEND-4/5) · backlog cleanup

Backlog item: saving a draft dropped its attachments. Persist them so a resumed draft keeps its files.

## In scope
- Store: migration #11 `draft_attachment` (draft_id FK CASCADE, filename, content_type, data BLOB —
  encrypted at rest like the rest of the DB); `DraftAttachment`; `replace_draft_attachments(draft_id,
  &[..])` (whole-set replace in one tx) + `draft_attachments(draft_id)`.
- App: on Save draft, persist the composed attachments alongside the draft; on Resume, restore them
  into the compose tray. (Save also now uses the in-view account.)

## Out of scope
- Streaming/large-attachment limits; dedup of identical blobs across drafts.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. Round-trip (order + bytes), whole-set replace, clear, and cascade-on-draft-delete tested; store
   mutants 0-missed. Save→resume keeps attachments (maintainer eyeballs the tray).

## Deliverables
- migration #11 + `DraftAttachment` + replace/get + tests; save/resume wiring.
