# ADR-0008: Encryption at rest via SQLCipher

## Status
Accepted (slice S2.2). Decided by the maintainer at the M2 encryption fork.

## Context
SEC-1 requires the local mailbox encrypted at rest, with **transparent unlock** (no master
password) — the key held in the OS keychain (SEC-2 / S2.1). Two approaches were weighed
(see S2.1 follow-up / the M2 plan):

- **Pure-Rust, application-level** AEAD on selected columns — consistent with ADR-0006's
  rustls/ring "no OpenSSL" choice, but bespoke, partial (metadata/index harder), and we own the
  envelope format.
- **SQLCipher** — transparent, whole-database AES-256 (pages, indexes, everything), battle-tested,
  but links **OpenSSL (C)**.

## Decision
Use **SQLCipher** (whole-database transparent encryption). `geleit-store` builds `rusqlite` with
`bundled-sqlcipher-vendored-openssl`; the database is opened with `PRAGMA key` from a per-install
**32-byte random key held in the OS keychain** (generated on first run). Nothing is plaintext on
disk, and unlock is transparent.

- **OpenSSL is vendored** (built from source, statically linked) → **no system dependency**, in
  keeping with our bundled-everything posture (bundled SQLite, webpki-roots).
- This is a **scoped exception** to ADR-0006: TLS for the network stays **rustls + ring** (no
  OpenSSL on the wire). OpenSSL enters only as SQLCipher's at-rest cipher.
- **Key management:** `geleit-app` generates a 32-byte key with `getrandom` on first run, stores it
  in the keychain (`OsSecretStore`), and passes it to `Store::open_encrypted`. The key never
  touches the database or logs (P2).

## Consequences
- The whole local store is encrypted; losing the keychain entry means the DB can't be opened
  (acceptable — it's a local cache; re-add the account to re-sync).
- CI builds OpenSSL from source (needs a C toolchain + perl, both present on the runners) → a few
  minutes slower. No `cargo-deny` change needed: `openssl-sys`/`openssl-src`/`libsqlite3-sys`
  declare MIT / MIT-OR-Apache-2.0, already in the allowlist.
- An older *unencrypted* dev database won't open with a key (and vice-versa); pre-S2.2 dev DBs must
  be deleted. (No released data exists yet.)
- `zeroize` of the key buffer in memory is a follow-up (guidelines §9; the key is short-lived).
- Escape hatch: if the OpenSSL footprint becomes a problem, revisit with a pure-Rust SQLite cipher
  or app-level AEAD.
