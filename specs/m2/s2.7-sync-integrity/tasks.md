# S2.7 — Sync integrity (property tests) · Tasks

Derived from `spec.md` + `plan.md` (P8). Testing/hardening slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] `proptest` dev-dependency on geleit-engine
- [x] `reconcile` property tests: set identities, disjoint, no-dupes, convergence (no loss/extra),
      idempotent, **interrupt-then-resume** convergence; duplicate-input-robust
- [x] hardened `reconcile` to dedup its output (set difference) so dup inputs can't yield dup work

## Verify (acceptance criteria)
- [x] AC1 build/test/clippy/fmt/`cargo deny check` green (proptest licenses pass)
- [x] AC2 properties pass + are non-trivial (mutants confirm: 36/36 caught); live incremental +
      backfill still pass after the reconcile change
- [x] AC3 `cargo mutants -p geleit-engine` — 36 caught / 0 missed

## Ship
- [x] Code review (guidelines §11, test-focused) — verdict sound + meaningfully constrains reconcile.
      Acted on findings: "no dupes" is now genuinely pinned (and reconcile dedups its output);
      added an explicit interrupt-then-resume property (was prose-only). Convergence-redundant-given-
      identities left as a readable spec statement.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)