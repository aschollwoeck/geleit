# S0.4 — Finalize the UI-framework decision · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md` (P8). Documentation-only slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Do
- [x] ADR-0001 status → **Accepted** (cites both gate results; points to guidelines §13)
- [x] Finalize `guidelines.md` §13 (dropped "provisional"; Slint conventions + spike rules:
      virtualized ListView, sandboxed webview + sanitize + JS-disable + CSS-aware sanitizer,
      GPU-preferred backend, state flow, a11y, theming) — and updated the stale intro note
- [x] Consistency pass (no "provisional" left on §13/intro; ADR=Accepted; cross-refs agree)

## Verify (acceptance criteria)
- [x] AC1 ADR-0001 = Accepted
- [x] AC2 §13 no longer provisional; states Slint + spike-derived rules
- [x] AC3 cross-references consistent (ADR ↔ S0.2/S0.3 findings ↔ guidelines §13)

## Ship
- [x] Review (guidelines §11): self consistency pass for a doc-only decision slice (grep for
      stale markers + cross-reference check + workspace still builds); no code/tests/manual
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)
