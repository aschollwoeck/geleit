# S0.5 — Platform-abstraction seams · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md` (P8). Real workspace code; full guidelines apply.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] `crates/geleit-platform` crate (Cargo.toml, workspace lints, no third-party deps)
- [x] `secret` — `SecretStore` trait + `SecretError` + `InMemorySecretStore` + tests
- [x] `oauth` — `OAuthRedirect` trait + `OAuthError` + `FakeOAuthRedirect` + test
- [x] `html` — `HtmlRenderHost` trait + `RenderError` + `NullHtmlRenderHost` + test
- [x] Add `geleit-platform` to workspace members
- [x] `geleit-engine` → depends on `geleit-platform`; tested `SecretStore` placeholder
- [x] Update `scripts/check-boundary.sh` (platform must not depend on app)
- [x] Update `.github/workflows/ci.yml` mutants-diff to include `geleit-platform`

## Verify (acceptance criteria — measurable)
- [x] AC1 `cargo build/test --workspace` green (15 tests); `clippy -D warnings` clean; `fmt --check` clean
- [x] AC2 each trait: API documented, typed error, tested double (incl. Display tests)
- [x] AC3 engine uses a seam (tested); boundary check passes
- [x] AC4 `cargo mutants --package geleit-platform`: 11/11 caught (3 Display mutants missed on
      first run → added error-Display tests → all caught); CI updated
- [x] AC5 ADR-0004 recorded; workspace doc updated

## Document
- [x] `docs/adr/0004-platform-abstraction-seams.md`
- [x] `docs/technical/workspace.md` updated (crate list + dependency direction)
- [x] (No end-user manual — infrastructure slice)

## Ship
- [x] Code review of the slice diff (guidelines §11) — 1 agent; no bugs. Acted on two
      future-proofing flags via documented "provisional" notes (ADR-0004 + trait docs): the
      secret-byte type becomes a zeroizing wrapper in M1 (§9); `OAuthRedirect` widens for
      CSRF state + PKCE in M7. Added a doc note on why `HtmlRenderHost` omits `Send + Sync`.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)
