# S1.5 — Sync a folder's recent envelopes · Spec (the WHAT)

Slice of **M1** (`roadmap.md`). Type: engine/integration. Delivers **SYNC-1 (basic)** and
**ACC-4 (partial)** — recent mail appears locally. No end-user manual (no UI yet, S1.7).

Status: **draft.**

## Purpose
Fetch a folder's recent message **envelopes** (UID, subject, from, date, seen flag) over IMAP and
store them, so the local store has message headers to show (P1). Naive (a recent window, not
incremental — that's M2). Verified against the local Dovecot by appending a known message and
syncing it back.

## In scope
- `geleit-store`: a `NewMessage` input + `upsert_message` (idempotent on `(account, folder, uid)`,
  updates flags/fields on re-sync) and a `messages_in_folder` read (newest-first, the index).
- `geleit-engine`: an `envelope` module of **pure** decode/format helpers (mutation-tested), and
  `sync_envelopes` (select folder → FETCH recent envelopes → map → upsert), plus a small
  `connect` helper refactored out of `list_folders`.
- Live verification: append a message to INBOX via IMAP, sync, assert it's in the store.

## Out of scope
- Message **bodies** / MIME decoding (S1.6) — RFC2047 subject decoding included. Snippets/
  attachment detection (need bodystructure/body) — `has_attachments` stays false, `snippet` none.
- Incremental sync / CONDSTORE (M2). UI (S1.7).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. **Live (`#[ignore]`, `--features dangerous-tls`):** append a known-subject message to INBOX →
   `sync_envelopes("INBOX")` → `messages_in_folder` contains that subject.
3. Unit (CI): `upsert_message` insert + re-upsert (same uid) updates, not duplicates;
   `messages_in_folder` newest-first + folder-scoped; `envelope` helpers (decode/address) correct.
4. `cargo mutants` on touched crates runs/reports (store + the pure `envelope` module covered;
   `imap.rs` network code excluded).
5. — (no new ADR; uses ADR-0006 stack.)

## Deliverables
- `geleit-store` (NewMessage, MessageHeader, upsert_message, messages_in_folder + tests).
- `geleit-engine/src/envelope.rs` (pure helpers + tests); `imap.rs` (`connect`, `sync_envelopes`).
- *(No end-user manual.)*
