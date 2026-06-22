# S8.3 — Calm/fast final pass (APP-2) · Spec

## In scope
- Tighten the release profile: `lto = "fat"` + `codegen-units = 1` (kept `strip`; deliberately not
  `panic = "abort"` — workers need `catch_unwind`). Binary ~32 MB → ~26 MB.
- Document the performance posture (`docs/technical/performance-notes.md`): off-UI-thread workers,
  optimistic actions, synchronous-instant FTS search, virtualized list, in-place row updates, lazy
  webview.

## Out of scope
- GUI-launched RAM/startup profiling (the harness can't run the GUI — a maintainer step); CONDSTORE
  incremental sync; background multi-account sync.

## Acceptance criteria
1. Release builds with the new profile; binary smaller; all gates green.
2. Perf posture documented.

## Deliverables
- `[profile.release]` tune; `docs/technical/performance-notes.md`.
