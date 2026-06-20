# S0.5 — Platform-abstraction seams · Spec (the WHAT)

Slice of **M0** (`roadmap.md`). Type: **infrastructure** — no user stories, so acceptance is
measurable pass/fail; no end-user manual (guidelines §11). This is real workspace code, held to
full guidelines. References ADR-0001; produces ADR-0004.

Status: **draft.**

## Purpose
Cross-platform is a constitution goal but only Linux is the dev target now (roadmap). To stop
that becoming deferred risk, define **trait-level seams** for the components that are implemented
differently per OS or per UI host, so engine/UI code depends on **interfaces**, not platforms.
Real per-OS implementations land in later milestones; this slice establishes the boundary and
provides testable doubles.

## The three seams (from the roadmap)
1. **Secret storage (keychain)** — store/fetch secrets (the at-rest key, credentials, OAuth
   tokens). Real backends later: Secret Service/libsecret (Linux), Keychain (macOS), Credential
   Manager (Windows).
2. **OAuth loopback** — desktop OAuth: open the auth URL, capture the localhost redirect, return
   the authorization code. Real impl M7 (browser launch differs per OS).
3. **HTML render host** — the UI host that displays *already-sanitized* email HTML in the
   sandboxed webview (ADR-0001). Real impl M3/M4 in the UI crate.

## In scope
- A new UI-agnostic crate **`geleit-platform`** with the three traits above, each with a typed
  error and a **testable double** (in-memory secret store; fake OAuth; no-op render host).
- Wire **`geleit-engine`** to depend on `geleit-platform` (engine depends only on the traits).
- Extend the engine/UI **boundary check** and CI mutation testing to cover the new crate.
- ADR-0004 recording the seam design; update the workspace technical doc.

## Out of scope
- Any real OS implementation (keychain/loopback/webview) — later milestones.
- Third-party crates: keep `geleit-platform` **dependency-free** (hand-rolled error types) so the
  M0 "zero third-party deps" state holds; `thiserror` + supply-chain CI (`cargo deny`/`audit`,
  guidelines §6) arrive together with the first real dependency in M1.

## Acceptance criteria (measurable)
1. `geleit-platform` builds; `cargo build/test --workspace` green; `clippy -D warnings` clean.
2. Each trait has a documented API, a typed error, and a tested double (round-trip / behavior).
3. `geleit-engine` depends on `geleit-platform` and uses a seam (a tested placeholder via
   `SecretStore`); the boundary holds (no engine/core/platform → UI dependency).
4. `cargo mutants` covers `geleit-platform` (CI + boundary check updated).
5. ADR-0004 recorded; workspace technical doc lists the new crate.

## Deliverables
- `crates/geleit-platform/` (traits + errors + doubles + unit tests).
- Updated root `Cargo.toml` (member), `scripts/check-boundary.sh`, `.github/workflows/ci.yml`.
- `docs/adr/0004-platform-abstraction-seams.md`; updated `docs/technical/workspace.md`.
- *(No end-user manual — infrastructure slice.)*
