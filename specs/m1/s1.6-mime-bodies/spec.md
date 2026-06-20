# S1.6 — Fetch + MIME-parse plaintext bodies · Spec (the WHAT)

Slice of **M1** (`roadmap.md`). Type: engine/integration. Backs READ-3 (read plaintext) by
storing message bodies; fills `snippet` (READ-2) and `has_attachments` (READ-2 indicator).

Status: **draft.**

## Purpose
Fetch full message bodies over IMAP, **MIME-parse** them (`mail-parser`) to extract the plaintext
(and HTML) body, a short snippet, and whether the message has attachments, and store them — so the
reading pane (S1.7) and list (snippet/clip) have real content from local data (P1).

## In scope
- A pure `mime` module in `geleit-engine`: `parse_body(&[u8]) -> ParsedBody` (plain, html, snippet,
  has_attachments) + `make_snippet` — unit- and mutation-tested.
- `geleit-store`: `store_body` (write the `body` row + update the message's snippet/has_attachments,
  atomically), `message_id_by_uid`, and a `body_for` read.
- `geleit-engine::imap::sync_bodies` — fetch `BODY.PEEK[]` for the recent window, parse, store
  (matched to the stored message by UID; envelopes must be synced first, S1.5).
- Live verification: append a multipart (text+html+attachment) message, sync, assert body + snippet
  + has_attachments.

## Out of scope
- Safe HTML rendering (M3, S0.2 spike). RFC2047 is handled by `mail-parser` for the body, but
  envelope-header decoding stays as S1.5's lossy pass. Incremental/large-mailbox sync (M2).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green (mail-parser licenses pass).
2. **Live (`#[ignore]`, `--features dangerous-tls`):** append a multipart message → sync envelopes
   + bodies → the stored message has the plaintext body, a snippet, and `has_attachments = true`.
3. Unit (CI): `parse_body` on a crafted multipart (plain/html/attachment); `make_snippet`
   collapses whitespace + truncates; `store_body` writes body + updates message fields (atomic);
   `message_id_by_uid`/`body_for` correct.
4. `cargo mutants` (store + `mime` module covered; `imap.rs` excluded) — 0 missed.
5. — (no new ADR.)

## Deliverables
- `geleit-engine/src/mime.rs`; `imap::sync_bodies`; `geleit-store` body methods. *(No end-user manual.)*
