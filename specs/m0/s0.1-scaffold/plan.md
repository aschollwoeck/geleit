# S0.1 — Workspace scaffold + CI · Plan (the HOW)

Implements `spec.md`. References ADR-0001, ADR-0002; produces ADR-0003.

## Toolchain
- **Rust 1.96.0 stable**, **edition 2021**. Pinned in `rust-toolchain.toml`.

## Workspace layout
```
Cargo.toml                 # [workspace], resolver = "2", shared lints
rust-toolchain.toml        # pin 1.96.0 + rustfmt, clippy components
crates/
  geleit-core/             # lib — pure domain types, UI-agnostic, no deps (mutants target)
  geleit-engine/           # lib — engine facade; depends on geleit-core
  geleit-app/              # bin — app entrypoint (future Slint shell); deps: engine + core
.github/workflows/ci.yml
docs/adr/0003-workspace-crate-structure.md
docs/technical/workspace.md
```
Each crate ships a trivial placeholder function + unit test so build/test/mutants have real
input. **Slint is *not* added in this slice** — `geleit-app` is a plain placeholder binary;
the Slint dependency arrives with the UI spike (S0.3) to keep this slice scoped to scaffold+CI.

## Boundary enforcement (engine/core must not depend on UI)
- Dependency direction is `geleit-app → geleit-engine → geleit-core`. Cargo **forbids
  dependency cycles**, so `engine`/`core` *cannot* depend on `app` — the boundary is enforced
  by construction.
- Belt-and-suspenders CI step (`scripts/check-boundary.sh`): use `cargo tree` to assert
  `geleit-app` is absent from the dependency subtrees of `geleit-core` and `geleit-engine`;
  fail the job otherwise. (Uses only `cargo` — no `jq`/extra tooling dependency.)

## CI (`.github/workflows/ci.yml`, GitHub Actions — ADR-0002)
- **Triggers:** `pull_request`; `push` to `main`; `schedule` (nightly) for the full mutants run.
- **Job `lint-test`** (PR + push), `ubuntu-latest`:
  1. checkout · install toolchain (honors `rust-toolchain.toml`) · `Swatinem/rust-cache`
  2. `cargo fmt --all --check`
  3. `cargo clippy --workspace --all-targets -- -D warnings`
  4. `cargo test --workspace`
  5. boundary assertion (cargo metadata + jq)
- **Job `mutants-diff`** (PR only): install `cargo-mutants`, run `cargo mutants --in-diff`
  against the PR diff (`git diff origin/main...HEAD`). Scoped to the diff so PRs stay fast.
- **Job `mutants-nightly`** (schedule): `cargo mutants --package geleit-core` (full core run).
- Thresholds are **not** gated yet (run-and-report only); tuned in a later slice.

## Verification (local, before PR)
Run criteria 1–5 locally: `cargo build/test/fmt --check/clippy`, the jq boundary check, and
`cargo mutants --package geleit-core`. Criterion 6 (CI passes green / fails on breakage) is
verified by pushing the branch and, once, a throwaway broken commit, then reverting.

## ADRs produced
- **ADR-0003** — workspace/crate structure: the `core`/`engine`/`app` split, `geleit-` naming,
  `crates/` layout, and the Cargo-cycle + metadata boundary enforcement.
