# Technical: workspace layout & CI

Scaffold established in slice **S0.1**. See ADR-0002 (CI), ADR-0003 (crate structure).

## Crates
- **`geleit-core`** — UI-agnostic domain types. No dependencies. Mutation-testing target.
- **`geleit-engine`** — engine facade; depends on `geleit-core`. Future home of
  store / sync / MIME / search / transport / auth.
- **`geleit-app`** — binary entrypoint; depends on `geleit-engine` + `geleit-core`. Becomes the
  Slint shell (ADR-0001) in S0.3.

Direction: `app → engine → core`. The reverse is impossible (Cargo's no-cycle rule); a CI
check (`scripts/check-boundary.sh`) asserts it explicitly.

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
