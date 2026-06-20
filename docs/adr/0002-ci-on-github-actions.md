# ADR-0002: Continuous integration on GitHub Actions

## Status
Accepted.

## Context
The repository is hosted on GitHub. Every slice PR must be gated (guidelines §11–12) on
formatting, lints, tests, and mutation testing before merge.

## Decision
Use **GitHub Actions** for CI. The pipeline gates each PR on:
- `cargo fmt --check`
- `cargo clippy -D warnings`
- `cargo test`
- `cargo mutants` on touched core crates (thresholds tuned in the S0.1 slice plan).

## Consequences
- CI configuration lives in `.github/workflows/`.
- Wired up in slice **S0.1** (scaffold); thresholds and the OS matrix (cross-platform at M8)
  evolve from there.
