# S2.2 — Encryption at rest · Spec (the WHAT)

Slice of **M2**. Type: store + app + supply-chain. Delivers **SEC-1**: the local mailbox is
encrypted at rest with a key held in the OS keychain — transparent unlock, no master password.
Approach: **SQLCipher** (ADR-0008).

Status: **draft.**

## Purpose
Everything GeleitMail stores on disk (message bodies, subjects, senders, indexes — the whole DB) is
encrypted. The key lives in the OS keychain (S2.1) and is applied automatically at open, so the
person never types a passphrase.

## In scope
- `geleit-store`: `rusqlite` switched to `bundled-sqlcipher-vendored-openssl`; `open_encrypted(path,
  key)` applies `PRAGMA key` before any access. (Vendored OpenSSL → no system dep.)
- `geleit-app`: a per-install 32-byte random DB key, generated with `getrandom` on first run and
  stored in the keychain; `open_store(db_path, secrets)` (get-or-create key → `open_encrypted`).
  All store opens (UI thread + sync/setup/refresh/backfill/remove workers) go through it.
- `cargo-deny`: allow the OpenSSL licenses; an ADR (0008) records the decision.

## Out of scope
- Re-keying / key rotation; migrating an existing plaintext DB (none released — old dev DBs are
  deleted). `zeroize` of the key buffer (follow-up). Encrypting the keychain itself (that's the OS's
  job). TLS stays rustls/ring (ADR-0006 unchanged).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green (OpenSSL licenses allowed).
2. **Encryption proven (deterministic, no keychain):** `open_encrypted` with a key round-trips;
   reopening with the **wrong key fails**; opening the same file **unencrypted fails** — so the
   file is genuinely ciphertext, not plaintext.
3. `db_key(secrets)` generates a 32-byte key once and returns the **same** key thereafter (tested
   with `InMemorySecretStore`); `open_store` opens an encrypted DB through it.
4. The app + all sync workers open the DB encrypted; no key/PII in logs or errors (P2).
5. `cargo mutants` — store/new logic covered as applicable; `imap.rs`/`refresh.rs`/`os_secret.rs`
   excluded; 0 missed.

## Deliverables
- `Store::open_encrypted` + encryption test; `geleit-app` key management (`db_key`/`open_store`) +
  wiring + tests; `deny.toml` license updates; ADR-0008; `docs/manual/` privacy note.
