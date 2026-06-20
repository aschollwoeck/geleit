# S2.2 — Encryption at rest · Plan (the HOW)

Implements `spec.md` per ADR-0008.

## geleit-store
- Dep: `rusqlite` features `["bundled-sqlcipher-vendored-openssl"]` (was `bundled`).
- `open_encrypted<P: AsRef<Path>>(path, key: &[u8]) -> Result<Self, StoreError>`: open the
  connection, run `PRAGMA key = "x'<hex(key)>'"` **first** (SQLCipher requires the key before any
  other statement), then `init` (foreign_keys + migrate). A wrong key surfaces as a `Sqlite` error
  on the first read (in `migrate`). `open`/`open_in_memory` stay (unencrypted; tests/dev).
- Test (deterministic, no keychain): write with key K → reopen with K reads it back; reopen with a
  different key → `Err`; `open` (no key) on the same file → `Err` (it's ciphertext).

## geleit-app
- Dep: `getrandom`.
- `refresh::db_key(secrets: &dyn SecretStore) -> Result<Vec<u8>, String>`: `get(DB_KEY_SERVICE,
  DB_KEY_ACCOUNT)`; if a 32-byte key exists, return it; else `getrandom` 32 bytes, `set`, return.
- `refresh::open_store(db_path, secrets) -> Result<Store, String>`: `db_key` → `open_encrypted`.
- Replace every `Store::open(db_path)` with `open_store(db_path, secrets)` in `run_setup`,
  `run_refresh`, `run_backfill`, `run_remove_account`, and `main` (startup). Workers already hold
  the shared secrets; `main` uses `&*secrets`.
- Tests: `db_key` returns 32 bytes + is stable across calls (`InMemorySecretStore`); `open_store`
  opens an encrypted DB over a temp file.

## Supply-chain
- `deny.toml`: add the OpenSSL licenses surfaced by the vendored build (run `cargo deny check` and
  add exactly what it flags — likely `OpenSSL` and/or `Apache-2.0`-family for `openssl`/`openssl-src`).

## Verify
build/test/clippy/fmt/`cargo deny check`; the encryption + key tests; live engine/app tests still
pass (they use unencrypted `open` in tests); app launches (creates encrypted DB + keychain key).
`cargo mutants`.
