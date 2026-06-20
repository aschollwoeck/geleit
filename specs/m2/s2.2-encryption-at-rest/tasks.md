# S2.2 — Encryption at rest · Tasks

Derived from `spec.md` + `plan.md` (P8) + ADR-0008. Store + app + supply-chain slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] store: rusqlite → `bundled-sqlcipher-vendored-openssl`; `open_encrypted` (PRAGMA key first) + test
- [x] app: `getrandom` dep; `refresh::db_key` (get-or-create 32-byte key) + `open_store` + tests
- [x] wire `main` + run_setup/run_refresh/run_backfill/run_remove_account to `open_store`
- [x] ADR-0008; no `deny.toml` change needed (openssl crates already MIT/Apache in the allowlist)
- [x] `docs/manual/` privacy note (encrypted at rest)

## Verify (acceptance criteria)
- [x] AC1 build/test/clippy/fmt/`cargo deny check` green
- [x] AC2 encryption proven: round-trip ok; **wrong key fails; plaintext open fails**; live app db
      has no plaintext SQLite header (random ciphertext)
- [x] AC3 db_key 32 bytes + stable; open_store opens encrypted
- [x] AC4 app + all workers open encrypted; no key/PII in errors (key never leaks via the PRAGMA SQL)
- [x] AC5 mutants store+app: 56 caught / 10 unviable / 0 missed

## Ship
- [x] Code review (guidelines §11) — verdict sound, **key does not leak** via error messages
      (PRAGMA hex always valid SQL; store errors discarded at the app boundary). Fixed the one real
      bug: `db_key` no longer regenerates/overwrites the key on a keychain read error or corrupt
      entry (would have bricked the DB) — it now only generates when the entry is genuinely absent.
      Corrected the ADR's deny-allowlist sentence; added a defensive comment in `open_encrypted`.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12) — unblocks the rest of M2