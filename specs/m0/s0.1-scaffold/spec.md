# S0.1 — Workspace scaffold + CI · Spec (the WHAT)

Slice of **M0** (`roadmap.md`). Type: **infrastructure** — no user stories, so acceptance is
measurable pass/fail criteria and there is no end-user manual (guidelines §11).
References: ADR-0001 (Slint + sandboxed webview), ADR-0002 (CI = GitHub Actions).
Produces: ADR-0003 (workspace/crate structure).

Status: **draft.**

---

## Purpose

Make the repository buildable with the **engine/UI boundary enforced**, and stand up **CI that
gates every future slice PR**. Everything else in the project builds on this slice.

## In scope

- A **Cargo workspace** that builds on Linux.
- `rust-toolchain.toml` pinning a stable Rust version.
- An **initial crate skeleton** that establishes the UI-agnostic boundary (guidelines §2):
  enough crates to *demonstrate* the boundary — a `core`/domain crate, one engine crate, and
  the UI crate. Further crates (`store`, `sync`, `mime`, …) are added by later slices as needed.
- **The boundary holds:** engine/core crates contain no UI types and do not depend on the UI crate.
- **GitHub Actions CI** gating on: `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test`, and `cargo mutants` (configured and runnable).
- A trivial placeholder (a function + its test) per crate so build/test/mutants have something
  real to run.

## Out of scope

- Any real functionality — sync, store, MIME, UI features (placeholders only).
- The full crate set (only the skeleton proving the boundary).
- Cross-platform CI matrix (Linux only now; Windows/macOS at M8).
- Mutation-testing **threshold** tuning (configured and runnable now; thresholds later).
- **Supply-chain CI** (`cargo deny` / `cargo audit`, guidelines §6) — **deferred** to the first
  slice that introduces a third-party dependency (early M1). Inert now: the scaffold has zero
  third-party dependencies, so there is nothing to audit yet.

## Acceptance criteria (measurable)

1. `cargo build --workspace` succeeds on Linux with the pinned toolchain.
2. `cargo test --workspace` runs and passes (placeholder tests).
3. `cargo fmt --check` passes, and `cargo clippy --workspace -- -D warnings` is clean.
4. `cargo mutants` runs against the core crate(s) and completes with a report.
5. The engine/UI boundary is enforced: an engine/core crate declaring a dependency on the UI
   crate fails the build or a CI check.
6. On a PR, CI runs all gates and **passes on green / fails on a deliberately broken change**
   (verified by a throwaway broken commit during development).
7. ADR-0003 (workspace/crate structure) is recorded.

## Deliverables

- Workspace `Cargo.toml`, member crates, `rust-toolchain.toml`.
- `.github/workflows/ci.yml`.
- Placeholder lib + tests per crate.
- `docs/adr/0003-workspace-crate-structure.md`.
- `docs/technical/` entry documenting the workspace layout and CI.
- *(No end-user manual — infrastructure slice.)*

## Open questions for the plan (`plan.md`)

1. Exact crate **names** and how many at scaffold (keep the skeleton minimal).
2. Which **Rust toolchain version** to pin.
3. **How to enforce** the engine→UI no-dependency rule (plain convention, `cargo-deny`, or a
   dedicated CI check).
4. `cargo mutants` **scope** and cadence — every PR vs. nightly (it is slow).
