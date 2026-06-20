# S0.1 — Workspace scaffold + CI · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md`. Kept current; this is the hand-off/status surface (P8).
Status legend: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] Workspace `Cargo.toml` (`[workspace]`, resolver 2, shared lints)
- [x] `rust-toolchain.toml` pinning 1.96.0 + rustfmt/clippy components
- [x] `crates/geleit-core` — placeholder fn + unit tests (no deps)
- [x] `crates/geleit-engine` — depends on core; placeholder using a core type + tests
- [x] `crates/geleit-app` — bin; depends on engine + core; `main` + a testable fn + test
- [x] `.github/workflows/ci.yml` — lint-test, mutants-diff (PR), mutants-nightly (schedule)
- [x] Boundary assertion script (`cargo tree`, no jq) wired into CI

## Verify (acceptance criteria)
- [x] AC1 `cargo build --workspace` succeeds (Linux, pinned toolchain)
- [x] AC2 `cargo test --workspace` passes (7 tests: core 4, engine 2, app 1)
- [x] AC3 `cargo fmt --all --check` clean; `cargo clippy --workspace -- -D warnings` clean
- [x] AC4 `cargo mutants --package geleit-core` runs and reports (13 mutants, 13 caught)
- [x] AC5 boundary holds (engine/core cannot depend on app — cycle + cargo-tree check)
- [~] AC6 CI passes on green and fails on a deliberately broken commit (verify on the PR)
- [x] AC7 ADR-0003 recorded

## Document
- [x] ADR-0003 — workspace/crate structure
- [x] `docs/technical/workspace.md` — layout + CI overview
- [x] (No end-user manual — infrastructure slice)

## Ship
- [ ] Code review of the slice diff (guidelines §11)
- [ ] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)
