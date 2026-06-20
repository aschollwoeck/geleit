# S2.4 — Progressive backfill · Tasks

Derived from `spec.md` + `plan.md` (P8). Engine + UI-glue slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] engine: extract `fetch_envelopes_for`/`fetch_bodies_for` (+ `uid_set`); refactor
      `sync_folder_incremental` to use them (behaviour-preserving)
- [x] engine `imap::backfill_folder` (newest-first, batched, `on_batch` progress)
- [x] app `refresh::run_backfill`; wire into the refresh worker (phase-1 incremental → phase-2
      backfill streaming "Catching up… N"; list reloads after each phase)
- [x] live test (backfill fetches beyond the recent cap; idempotent; monotonic progress)

## Verify (acceptance criteria)
- [x] AC1 build/test/clippy/fmt/deny green
- [x] AC2 LIVE: incremental(limit=2) recent-only; backfill fetches the rest (envelopes+bodies)
- [x] AC3 `on_batch` monotonic; complete-folder backfill = 0 (no callback)
- [x] AC4 P1/P2 unchanged (worker thread, only Send crosses); status streams progress
- [x] AC5 mutants store+engine: 89 caught / 8 unviable / 0 missed (imap.rs excluded; reconcile/store covered)

## Ship
- [x] Code review (guidelines §11) — verdict sound: refactor behaviour-preserving, backfill correct
      (newest-first, no-zero-panic, idempotent, both envelopes+bodies, never deletes), threading safe
      (only Send crosses). Added the §5 NOTE to the shared fetch helpers + a UIDVALIDITY
      self-heal note in backfill. Remaining findings (selection reset twice, two runtimes) are
      pre-existing/harmless.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)