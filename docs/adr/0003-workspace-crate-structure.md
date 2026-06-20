# ADR-0003: Workspace and crate structure

## Status
Accepted.

## Context
The engine must be UI-agnostic (constitution P4/P6, guidelines §2): no UI type in engine code,
and no engine→UI dependency. We need an initial structure that enforces this and that later
slices extend (store, sync, MIME, search, transport, auth).

## Decision
A Cargo workspace under `crates/`, with a `geleit-` naming prefix:

- **`geleit-core`** — pure, UI-agnostic domain types; no dependencies; the mutation-testing target.
- **`geleit-engine`** — engine facade; depends on `geleit-core`; grows into store/sync/etc.
- **`geleit-app`** — the binary / future Slint shell; depends on `geleit-engine` + `geleit-core`.

Dependency direction is strictly **`app → engine → core`**. Because Cargo forbids dependency
cycles, `engine`/`core` *cannot* depend on `app` — the boundary is enforced by construction. A
CI check (`cargo tree`, `scripts/check-boundary.sh`) asserts it as belt-and-suspenders.

Shared settings live in `[workspace.package]` (edition 2021, MIT, rust-version 1.96) and
`[workspace.lints]` (`unsafe_code = forbid`, clippy `all = warn`); CI enforces `clippy -D warnings`.

## Consequences
- New crates join `crates/`, opt into workspace lints, and the engine/UI boundary stays structural.
- `geleit-app` remains a placeholder binary until the Slint UI lands (S0.3); **Slint is not a
  dependency of the scaffold**.
- Adding a future engine crate is additive; nothing depends on `app`.
