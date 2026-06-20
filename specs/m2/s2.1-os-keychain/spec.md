# S2.1 — Real OS keychain backend · Spec (the WHAT)

First slice of **M2**. Type: platform + integration. Delivers **SEC-2** for real (M1 shipped only
the seam + in-memory double). Replaces the in-memory secret store with the **OS keychain**, so
credentials persist securely across restarts — the foundation for encryption-at-rest (next slice)
and the fix for the password-re-entry limitation from S1.10.

Status: **draft.**

## Purpose
Store IMAP passwords (and, later, the at-rest key + OAuth tokens) in the operating-system keychain
via the existing `SecretStore` seam — so they survive restarts and never touch our database or logs.

## In scope
- `geleit-platform::os_secret::OsSecretStore`: a `SecretStore` over the OS keychain (Linux Secret
  Service / gnome-keyring, via `keyring` v4's pure-Rust zbus backend). Errors carry **no** secret
  or account material (P2).
- Wire `geleit-app` to use `OsSecretStore` instead of `InMemorySecretStore` → the password set at
  Add-account persists; after a restart, Refresh works without re-entering it.
- `run_setup`/`run_refresh` accept `&dyn SecretStore` (so either backend works; tests keep the
  in-memory double).

## Out of scope
- Encryption at rest (next slice, SEC-1). macOS/Windows keychain backends (M8 packaging — just
  enable their `keyring` store features then). `zeroize` of secret buffers (follow-up; ADR-0004).
- Incremental/background sync (later M2 slices).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green (keyring/zbus licenses pass).
2. **Live (`#[ignore]`, run where a secret service exists):** `OsSecretStore` set→get→update→delete
   round-trips; `get` of an absent key is `None`; `delete` is idempotent. (Verified against the
   local gnome-keyring.)
3. The app uses `OsSecretStore`; it launches; no secret/PII in any keychain error message (P2).
4. `cargo mutants` — `os_secret.rs` (external glue) excluded like `imap.rs`; `secret.rs`
   (`InMemorySecretStore`) stays covered; 0 missed.

## Deliverables
- `OsSecretStore` + live round-trip test; app wired to it; `docs/manual/` note (passwords are now
  remembered); roadmap M2 re-plan (keychain first). *(No new ADR — fulfils ADR-0004's seam.)*
