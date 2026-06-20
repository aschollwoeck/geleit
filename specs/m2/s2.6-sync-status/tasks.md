# S2.6 — Non-blocking sync status · Tasks

Derived from `spec.md` + `plan.md` (P8). UI slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] Slint: `sync-status` property + calm status strip (muted text + accent dot on surface, AA ≈5:1)
- [x] refresh worker: phases drive `sync-status` ("Checking for new mail…" → "Catching up… N" →
      cleared); backfill progress moved off the danger `status` banner; errors stay on `status`
- [x] brief manual touch (sync progress is calm)

## Verify (acceptance criteria)
- [x] AC1 build/test/clippy/fmt/`cargo deny check` green
- [x] AC2 calm sync-status during refresh/backfill; errors stay in the danger banner; no path leaks
      progress→danger or error→calm (review-traced); app launches with the strip
- [x] AC3 UI responsive (P1, unchanged — sync on worker, posts via invoke_from_event_loop)
- [x] AC4 mutants unaffected (UI plumbing in main.rs, excluded)

## Ship
- [x] Code review (guidelines §11) — verdict sound, separation correct on all paths, no stuck state.
      Acted on both Low findings: `reload_all` now also clears `sync-status` (symmetry); a backfill
      failure now surfaces a **calm** note ("Couldn't finish catching up — will resume next refresh")
      instead of being silently dropped (the message was dead code).
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)