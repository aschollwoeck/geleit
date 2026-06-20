# S1.6 — Fetch + MIME-parse plaintext bodies · Plan (the HOW)

Implements `spec.md`. Uses the ADR-0006 transport stack + `mail-parser`.

## geleit-engine::mime (pure, mutation-tested)
- `struct ParsedBody { plain: Option<String>, html: Option<String>, snippet: Option<String>,
  has_attachments: bool }` (`Default`).
- `parse_body(raw: &[u8]) -> ParsedBody`: `MessageParser::default().parse(raw)` → `body_text(0)`,
  `body_html(0)`, `attachment_count() > 0`; snippet from the plaintext via `make_snippet`. Empty
  `ParsedBody` if parsing fails.
- `make_snippet(text: &str, max: usize) -> String`: collapse whitespace (`split_whitespace`),
  take `max` chars (char-safe).

## geleit-store
- `store_body(message_id, plain, html, snippet, has_attachments)` — in an `unchecked_transaction`
  (atomic with `&self`): upsert the `body` row (`ON CONFLICT(message_id) DO UPDATE`) **and**
  `UPDATE message SET snippet, has_attachments`.
- `message_id_by_uid(account_id, folder_id, uid) -> Option<i64>`.
- `body_for(message_id) -> Option<(Option<String>, Option<String>)>` (plain, html) for tests/UI.

## geleit-engine::imap
- `sync_bodies(config, secrets, store, account_id, folder, limit) -> usize`: `connect` → `select`
  → `recent_window` → `fetch("(UID BODY.PEEK[])")` (PEEK so `\Seen` isn't set) → for each: parse
  `f.body()`, look up the message by `(account, folder, uid)`, `store_body`. Skip UID-less / bodyless.

## Tests
- Unit (CI): `parse_body` on a crafted `multipart/mixed`(`multipart/alternative`(text+html) +
  attachment) → plain/html/snippet present, `has_attachments`; `make_snippet` whitespace+truncate;
  store `store_body` atomic write + `body_for` + `message_id_by_uid`.
- Live (`#[ignore]`, `--features dangerous-tls`): append that multipart, `sync_envelopes` +
  `sync_bodies`, assert the stored message's body/snippet/has_attachments.

## Verify
build/test/clippy/fmt/`cargo deny check`; live `--features dangerous-tls -- --ignored`; mutants.
