# S0.4 — Finalize the UI-framework decision · Spec (the WHAT)

Slice of **M0** (`roadmap.md`). Type: **decision / documentation** — no user stories, no code,
no end-user manual (guidelines §11). References ADR-0001, and the S0.2 / S0.3 findings.

Status: **draft.**

## Purpose
M0's purpose was to commit to the native UI stack *with evidence*. Both ADR-0001 gates have now
passed — S0.2 (safe HTML rendering) and S0.3 (50k-row virtualized list). This slice records the
**final commitment to Slint** and turns the spike learnings into durable guidance, so M3+ build
on a settled, documented decision.

## In scope
- Move **ADR-0001** from *Proposed* to *Accepted*.
- Finalize **`guidelines.md` §13 (UI conventions)** — currently provisional pending M0 — with the
  committed framework and the spike-derived rules (sandboxed webview for HTML, sanitize +
  JS-disable, virtualized lists, GPU-preferred backend, state flow, theming, a11y).
- Ensure the carry-forward items for M3/M4 are durably recorded (CSS-aware sanitizer; explicit
  JS-disable; GPU backend) — they live in the ADR/findings; reference them where useful.

## Out of scope
- Any UI code (begins in M3).
- Building the CSS-aware sanitizer or JS-disable (M3/M4 — only recorded here).

## Acceptance criteria
1. ADR-0001 status = **Accepted**, citing both gate results.
2. `guidelines.md` §13 is no longer marked provisional and states the Slint conventions +
   spike-derived rules.
3. Cross-references are consistent (ADR ↔ findings ↔ guidelines).

## Deliverables
- Updated `docs/adr/0001-*.md` (Accepted).
- Finalized `guidelines.md` §13.
- *(No end-user manual; no code.)*
