# Technical: workspace layout & CI

Scaffold established in slice **S0.1**. See ADR-0002 (CI), ADR-0003 (crate structure).

## Crates
- **`geleit-core`** — UI-agnostic domain types. No dependencies. Mutation-testing target.
- **`geleit-platform`** — UI-agnostic trait seams for OS/external capabilities (keychain,
  OAuth loopback, HTML render host) with testable doubles (ADR-0004).
- **`geleit-store`** — the local SQLite store: account-scoped schema + migrations (ADR-0005).
  `rusqlite` (bundled SQLite). Encryption at rest comes in M2.
- **`geleit-engine`** — engine facade; depends on `geleit-core`, `geleit-platform`, `geleit-store`.
  Home of sync / MIME / search / transport / auth. The `imap` module connects over TLS
  (`async-imap` + `tokio` + `rustls`/`ring`, ADR-0006) and lists/persists folders.
- **`geleit-app`** — binary entrypoint; depends on `geleit-engine` + `geleit-core`. Becomes the
  Slint shell (ADR-0001).

Direction: `app → engine → {core, platform, store}`. The reverse is impossible (Cargo's no-cycle
rule); a CI check (`scripts/check-boundary.sh`) asserts the engine-side crates never depend on
the UI crate.

## Toolchain
Rust **1.96.0** stable (pinned in `rust-toolchain.toml`), edition 2021. `unsafe_code` is
forbidden in our crates.

## CI (GitHub Actions — ADR-0002)
`.github/workflows/ci.yml`:
- **`lint-test`** (PR + push to `main`): `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test`, boundary check.
- **`mutants-diff`** (PR): `cargo mutants --in-diff` on the PR diff — report-only.
- **`mutants-nightly`** (cron): full `cargo mutants` on `geleit-core` — report-only.

Mutation thresholds are **not gated yet** (tuned in a later slice).

## Local commands
```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/check-boundary.sh
cargo mutants --package geleit-core
```
