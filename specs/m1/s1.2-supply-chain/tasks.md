# S1.2 — First-dependency setup · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md` (P8). Infrastructure slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Do
- [x] `deny.toml` (advisories, MIT-compatible license allowlist, bans w/ allow-wildcard-paths, sources)
- [x] CI: `supply-chain` job runs `cargo deny check`
- [x] Migrate `geleit-platform` errors to `thiserror` (secret/oauth/html), messages unchanged
- [x] Update ADR-0004 note + geleit-platform Cargo.toml comment (thiserror adopted)

## Verify (acceptance criteria — measurable)
- [x] AC1 `cargo deny check` passes locally (exit 0; advisories/bans/licenses/sources ok)
- [x] AC2 CI `supply-chain` job runs cargo-deny on PRs/push
- [x] AC3 thiserror errors; Display tests pass; build/test (10)/clippy -D warnings/fmt green
- [x] AC4 `cargo mutants --package geleit-platform`: 8/8 caught
- [x] AC5 ADR-0004 + Cargo.toml comment updated

## Ship
- [x] Code review (guidelines §11) — 1 agent confirmed the gate is effective (v2 denies
      vuln/unsound; allow-wildcard-paths scoped to path deps only; thiserror byte-identical).
      Acted on findings: **pinned `cargo-deny@0.19.9`** in CI; documented that `unmaintained`
      is warn-only. Reviewer's `--all-features` suggestion verified invalid for cargo-deny
      (rejected, exit 2; it already covers the full graph) — not applied.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)
