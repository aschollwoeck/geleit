# S1.4 — Connect to one IMAP account & list folders · Plan (the HOW)

Implements `spec.md`. Produces ADR-0006.

## Transport stack (decided — ADR-0006)
- **`tokio`** runtime; **`async-imap`** (feature `runtime-tokio`); TLS via **`rustls`** with the
  **`ring`** crypto provider + **`tokio-rustls`**; `tokio_util::compat` bridges the tokio TLS
  stream to the futures-io traits async-imap wants. Real CA roots via **`webpki-roots`**.
- Chosen over native-tls because OpenSSL dev headers are absent and rustls/ring needs no system
  TLS lib (pure-Rust, integrity-aligned), and over rustls' default aws-lc-rs because `cmake` is
  absent (ring builds with cc+perl).

## `geleit-engine::imap`
- `ImapConfig { host: String, port: u16, username: String, allow_invalid_certs: bool }`.
- `ImapError` (`thiserror`) wrapping io / rustls / async-imap / store / secret errors + variants
  `NoPassword`, `NoGreeting`, `InvalidServerName`.
- Password comes from `&dyn SecretStore` (`get("geleit-imap", username)`); **never logged** (P2).
- `list_folders(&ImapConfig, &dyn SecretStore) -> Result<Vec<String>, ImapError>`:
  1. fetch password (→ `NoPassword` if absent — fails before any socket, so this path is
     unit-testable without a server);
  2. `TcpStream::connect`; build a rustls `ClientConfig` — `allow_invalid_certs` → a custom
     `ServerCertVerifier` that skips chain checks but still verifies handshake signatures (dev
     only); else webpki-roots; install the ring provider as process default once;
  3. `TlsConnector::connect`, `.compat()`, `async_imap::Client::new`, `read_response()` (greeting),
     `login`, `list(Some(""), Some("*"))`, collect `Name::name()`, `logout`.
- `persist_folders(&Store, account_id, &[String]) -> Result<(), StoreError>` — `upsert_folder`
  each (pure, unit-testable without a server).
- `sync_folders(...)` = `list_folders` + `persist_folders`.

## `geleit-store`
- Add `upsert_folder(account_id, name)` = `INSERT INTO folder ... ON CONFLICT(account_id,name)
  DO NOTHING` (idempotent re-sync), returning the row id.

## Tests
- **Unit (no network):** missing password → `NoPassword`; `persist_folders` idempotent +
  account-scoped (via in-memory store + InMemorySecretStore).
- **Live (`#[ignore]`, run locally):** `#[tokio::test]` connecting to `127.0.0.1:993`,
  `geleittest`/`testpass123` in an `InMemorySecretStore`, `allow_invalid_certs = true` → assert
  the returned folders contain `INBOX`. (CI can't reach Dovecot, so it's ignored there per §7.)

## Wiring & docs
- `geleit-engine` depends on `geleit-store`. Engine + store already covered by the boundary check
  and CI mutants. `cargo deny` vets the new licenses (add deliberately if any are new).
- ADR-0006 (transport stack); update `docs/technical/workspace.md`.

## Verify
`cargo build/test --workspace`, `clippy -D warnings`, `fmt`, `cargo deny check`,
`cargo test -p geleit-engine -- --ignored` (live, local), `cargo mutants` — green before PR.
