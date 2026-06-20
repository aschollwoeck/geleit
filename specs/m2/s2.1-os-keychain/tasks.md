# S2.1 — Real OS keychain backend · Tasks

Derived from `spec.md` + `plan.md` (P8). Platform + integration slice (first of M2).
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] platform: keyring dep (zbus secret-service); `OsSecretStore` (`SecretStore` impl) + live test
- [x] `lib.rs`: `pub mod os_secret;`
- [x] app: use `OsSecretStore`; `run_setup`/`run_refresh` take `&dyn SecretStore`
- [x] `.cargo/mutants.toml`: exclude `os_secret.rs`
- [x] `docs/manual/` note (passwords remembered); roadmap M2 re-plan

## Verify (acceptance criteria)
- [x] AC1 build/test/clippy/fmt/deny green
- [x] AC2 LIVE keychain round-trip passes against gnome-keyring (set/get/update/delete/absent/
      idempotent) — verified after pointing the default alias at the unlocked login keyring
- [x] AC3 app uses OsSecretStore + launches; keychain errors are fixed generic strings (no PII)
- [x] AC4 mutants platform+app: 13 caught / 1 unviable / 0 missed (os_secret.rs/refresh.rs/main.rs
      excluded); the pure `classify_get`/`classify_delete` arms are CI-unit-tested

## Ship
- [x] Code review (guidelines §11) — verdict sound (correct mapping, P2 clean, Send+Sync, CI-safe
      build). Acted on findings: extracted `classify_get`/`classify_delete` as CI-tested pure helpers
      (a swapped arm = silent credential loss, now caught); fixed the stale `main.rs` + engine
      `connect()` comments.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)