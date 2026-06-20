# S1.4 — Connect to one IMAP account & list folders · Spec (the WHAT)

Slice of **M1** (`roadmap.md`). Type: engine/integration. Delivers **ACC-3** (manual IMAP
config), **READ-6** (folders), and uses the **SecretStore** seam for credentials (SEC-2,
architecturally — the real OS-keychain backend is a separate follow-up). Produces ADR-0006.
No end-user manual (no user-facing UI yet — that's S1.7).

Status: **draft.**

## Purpose
First real network step: connect to an IMAP server over TLS, log in, **list the folders**, and
persist them to the local store so the UI (S1.7) can show them from local data (P1). Verified
end-to-end against a **local Dovecot** (`geleittest@127.0.0.1:993`).

## In scope
- An async IMAP layer in `geleit-engine` (`async-imap` over `tokio` + `rustls`/`ring`).
- `ImapConfig` — manual config (ACC-3): host, port, username, `allow_invalid_certs` (dev-only,
  off by default, for the local self-signed cert).
- Credentials fetched via the **`SecretStore` seam** (SEC-2); password never logged (P2).
- `list_folders` (connect → TLS → login → LIST → logout) and `persist_folders` (upsert into the
  store), composed as `sync_folders`. A store `upsert_folder` (INSERT-OR-IGNORE) for re-runs.
- ADR-0006 recording the IMAP/TLS transport stack.

## Out of scope
- Message sync / bodies (S1.5/S1.6); UI (S1.7).
- The **real OS-keychain backend** for `SecretStore` (still the in-memory double in tests; real
  backend is a dedicated follow-up — it needs `keyring`/Secret Service and a desktop session).
- Proper provider TLS/OAuth (M7). Real CA roots are wired (webpki-roots) for the non-dev path,
  but only the `allow_invalid_certs` localhost path is exercised this slice.

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green (new IMAP/TLS deps pass the gate).
2. **Live (local, `#[ignore]`d in CI):** `list_folders` against Dovecot
   (`geleittest`/`testpass123`, `allow_invalid_certs`) returns a list **containing `INBOX`**.
3. Non-network unit tests: missing password → `NoPassword` (no connection attempted);
   `persist_folders` upserts (idempotent; account-scoped) verified against the store.
4. `cargo mutants` on touched crates runs/reports.
5. ADR-0006 recorded.

## Deliverables
- `crates/geleit-engine/src/imap.rs` (+ deps); `geleit-store::upsert_folder`.
- `docs/adr/0006-imap-transport-stack.md`; updated `docs/technical/workspace.md`.
- *(No end-user manual.)*
