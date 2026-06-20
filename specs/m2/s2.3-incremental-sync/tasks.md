# S2.3 — Incremental sync · Tasks

Derived from `spec.md` + `plan.md` (P8). Engine + store slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] engine `sync::reconcile` (+ `SyncPlan`) + 6 tests
- [x] store: migration #3 (`uid_validity`); `uids_in_folder`, `delete_messages_by_uid`,
      `clear_folder`, `set_folder_uidvalidity`/`folder_uidvalidity`, `uids_without_body` + tests
- [x] engine `imap::sync_folder_incremental` (UIDVALIDITY reset, reconcile, delete, fetch new,
      self-healing body fetch)
- [x] wire `run_setup`/`run_refresh` to use it
- [x] live test (new appears, deleted removed, idempotent)

## Verify (acceptance criteria)
- [x] AC1 build/test/clippy/fmt/deny green
- [x] AC2 reconcile correct (6 tests: equal/new/deleted/both/empty-local/empty-server)
- [x] AC3 store uid methods + migration #3 tested
- [x] AC4 LIVE (`--features dangerous-tls`): append→sync→present; re-sync idempotent (no dupe);
      server delete→sync→removed locally
- [x] AC5 P1/P2 unchanged (sync on worker runtime; no PII in errors)
- [x] AC6 mutants store+engine: 89 caught / 8 unviable / 0 missed (reconcile + store covered;
      imap.rs excluded)

## Ship
- [x] Code review (guidelines §11) — verdict sound (reconcile correct, UIDVALIDITY handled, deletes
      folder-scoped, casts safe, atomic). Fixed the one real finding (P6): bodies are now fetched
      for any recent message lacking one (`uids_without_body`), so an interrupted body fetch
      self-heals — no permanent header-only messages.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)