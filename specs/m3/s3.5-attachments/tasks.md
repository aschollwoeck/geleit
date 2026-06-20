# S3.5 — Attachments (view) · Tasks

Derived from `spec.md` + `plan.md` (P8). Engine + store + UI slice (first of M3).
Status: `[x]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] mime: `Attachment` + `ParsedBody.attachments`; extract in `parse_body` + tests
- [x] store: migration #4 (`attachment` table); `Attachment`; `store_attachments`, `attachments_for` + tests
- [x] sync: store attachment metadata alongside the body (`fetch_bodies_for`)
- [x] app: `viewmodel::attachment_label` + tests; reading-pane attachments section
- [x] live test (synced attachment → `attachments_for` non-empty); manual touch

## Verify (acceptance criteria)
- [x] AC1 build/test/clippy/fmt/`cargo deny check` green
- [x] AC2 parse_body extracts attachments (multipart) / none (plain) — tested
- [x] AC3 store round-trip + replace + cascade — tested
- [x] AC4 attachment_label human size — tested
- [x] AC5 LIVE: synced message's attachment metadata in the store
- [x] AC6 mutants — mime/store/viewmodel covered; imap.rs/refresh.rs/main.rs excluded; 0 missed

## Ship
- [x] Code review (guidelines §11) — verdict sound, no blockers; added direct message-delete cascade assertion
- [x] Update this tasks file to all-done
- [x] PR merged (one-slice-one-PR, §12)