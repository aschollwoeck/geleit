# S1.5 — Sync a folder's recent envelopes · Plan (the HOW)

Implements `spec.md`. Uses the ADR-0006 transport stack.

## geleit-store
- `NewMessage { uid: Option<i64>, message_id, subject, from_name, from_addr, date: Option<i64>,
  seen: bool, has_attachments: bool, snippet }` (all owned/Option).
- `upsert_message(account_id, folder_id, &NewMessage) -> i64`: `INSERT ... ON CONFLICT(account_id,
  folder_id, uid) DO UPDATE SET <fields>` (refresh on re-sync). Return id (SELECT by uid when uid
  is `Some`; `last_insert_rowid` when `None`, since NULL uids never conflict).
- `MessageHeader { id, uid, subject, from_name, from_addr, date, seen, has_attachments }` and
  `messages_in_folder(folder_id, limit) -> Vec<MessageHeader>` ordered `date DESC, id DESC`
  (newest-first via the `message_folder_date` index).

## geleit-engine
- **`envelope` module (pure, mutation-tested):**
  - `decode_header(Option<&[u8]>) -> Option<String>` — lossy UTF-8 (RFC2047 MIME-word decoding is
    S1.6; documented).
  - `address_parts(name, mailbox, host: Option<&[u8]>) -> (Option<String>, Option<String>)` →
    `(from_name, "mailbox@host")`.
- **`imap` (network, live-tested, mutants-excluded):**
  - `type ImapSession = async_imap::Session<tokio_rustls::client::TlsStream<TcpStream>>`.
  - `connect(config, secrets) -> ImapSession` (factored out of `list_folders`; both use it).
  - `fetch_to_new_message(&Fetch) -> NewMessage` using the `envelope` helpers + `internal_date()`
    (`.timestamp()`), UID, and the `\Seen` flag.
  - `sync_envelopes(config, secrets, store, account_id, folder, limit) -> usize`:
    `upsert_folder` → `connect` → `select` → compute the last `min(limit, exists)` sequence window
    → `fetch("UID ENVELOPE FLAGS INTERNALDATE")` → `upsert_message` each → best-effort logout →
    return count.

## Tests
- Unit (CI): store upsert/update + messages_in_folder ordering/scoping; `envelope` decode +
  address formatting (incl. missing parts).
- Live (`#[ignore]`, `--features dangerous-tls`): `connect` + `append` a known message to INBOX,
  `sync_envelopes`, assert `messages_in_folder` contains the subject.

## Verify
`cargo build/test --workspace`, `clippy -D warnings`, `fmt`, `cargo deny check`,
`cargo test -p geleit-engine --features dangerous-tls -- --ignored` (live), `cargo mutants`.
