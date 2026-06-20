# S1.2 — First-dependency setup: supply-chain CI + thiserror · Spec (the WHAT)

Slice of **M1** (`roadmap.md`). Type: **infrastructure** — no user stories; measurable pass/fail,
no end-user manual (guidelines §11). References guidelines §4/§6, ADR-0004.

Status: **draft.**

## Purpose
M1 starts adding real third-party dependencies (rusqlite next, then async-imap, …). Per the
deferral recorded in S0.2/S0.5, the **supply-chain gate must be in place before they land**, and
`thiserror` (deferred from M0) is now adopted. This slice establishes that discipline once, so
every later dependency is checked from the moment it's added.

## In scope
- **`cargo-deny`** in CI (guidelines §6): a `deny.toml` and a CI check covering **advisories**
  (RustSec — same DB as `cargo audit`), **licenses** (MIT-compatible allowlist), **bans**, and
  **sources**. This satisfies the §6 "cargo deny/audit in CI" rule.
- **Adopt `thiserror`** (guidelines §4 "typed errors via thiserror"): migrate the three
  hand-rolled error enums in `geleit-platform` (`SecretError`, `OAuthError`, `RenderError`) to
  `thiserror`, keeping their messages/behavior identical.
- Update ADR-0004's "dependency-free" note (thiserror is now the first dependency).

## Out of scope
- The store schema / rusqlite (next slice, S1.3) — this slice only sets up the gate.
- Migrating any other errors (none exist yet beyond geleit-platform).

## Acceptance criteria (measurable)
1. `cargo deny check` passes locally (advisories, licenses, bans, sources) with `deny.toml`.
2. CI runs `cargo-deny` on PRs (and is wired into the existing workflow).
3. `geleit-platform` errors use `thiserror`; the existing error-`Display` tests still pass
   (messages unchanged); build + `cargo test --workspace` + `clippy -D warnings` + `fmt` green.
4. `cargo mutants` on `geleit-platform` still passes (no new survivors).
5. ADR-0004 note updated; the geleit-platform "dependency-free" comment corrected.

## Deliverables
- `deny.toml`; CI update (`.github/workflows/ci.yml`).
- `geleit-platform` migrated to `thiserror` (Cargo.toml + secret/oauth/html modules).
- Updated `docs/adr/0004-platform-abstraction-seams.md`.
- *(No end-user manual — infrastructure slice.)*
