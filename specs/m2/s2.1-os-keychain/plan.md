# S2.1 — Real OS keychain backend · Plan (the HOW)

Implements `spec.md`. Fulfils the `SecretStore` seam (ADR-0004) with a real backend.

## geleit-platform
- Dep: `keyring` v4, `default-features = false`, features `["v1", "zbus-secret-service-keyring-store"]`
  (pure-Rust zbus Secret Service — no C deps; Linux only for now).
- `src/os_secret.rs` (external glue — mutants-excluded): `OsSecretStore` (unit struct) implementing
  `SecretStore`:
  - `entry(service, account)` → `keyring::Entry::new` (auto-registers the platform store), mapping
    any error to `SecretError::Backend("keychain unavailable")` — **no** secret/account in the message.
  - `set` → `entry.set_secret`; `get` → `entry.get_secret` (`Err(NoEntry)` → `Ok(None)`); `delete`
    → `entry.delete_credential` (`NoEntry` → `Ok(())`, idempotent). Other errors → generic `Backend`.
  - Live `#[ignore]` round-trip test (set/get/update/delete/absent/idempotent-delete).
- `lib.rs`: `pub mod os_secret;`.

## geleit-app
- `main.rs`: `secrets = Arc::new(OsSecretStore::new())` (Send+Sync; shared into workers as before).
- `refresh.rs`: `run_setup`/`run_refresh` take `secrets: &dyn SecretStore` (was the concrete in-mem
  type); the engine `connect`/`store_password`/`has_password` already take `&dyn SecretStore`.

## Verify
gates; **run the live keychain test** here (`cargo test -p geleit-platform -- --ignored`, gnome-keyring
present); app launches; `cargo mutants` with `os_secret.rs` excluded. `.cargo/mutants.toml` updated.
Manual + roadmap M2 re-plan.
