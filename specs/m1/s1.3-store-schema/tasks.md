# S1.3 — Local store schema · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md` (P8). Real workspace code; full guidelines.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] `crates/geleit-store` (rusqlite bundled, thiserror, geleit-core) + workspace member
- [x] Schema migration 1: account / folder / message / body (account-scoped, FKs, indexes)
- [x] Migration runner (user_version, transactional, append-only)
- [x] `Store::open` / `open_in_memory` (foreign_keys ON + migrate); `StoreError` (thiserror, wraps rusqlite)
- [x] Account + folder ops (add/get/list) to exercise the schema
- [x] Boundary check + CI mutants extended to `geleit-store`

## Verify (acceptance criteria — measurable)
- [x] AC1 build/test (7 store tests)/clippy -D warnings/fmt/`cargo deny check` green
      (gate caught foldhash's Zlib license → added deliberately)
- [x] AC2 migrations create all tables; user_version set; second migrate idempotent (tested)
- [x] AC3 account-scoping: UNIQUE (email; account+folder) enforced; FK cascade delete verified
- [x] AC4 `cargo mutants --package geleit-store`: 11 caught, 6 unviable, 0 survived
- [x] AC5 ADR-0005 + workspace doc updated

## Document
- [x] `docs/adr/0005-local-store-schema.md`
- [x] `docs/technical/workspace.md` (crate list + dependency direction)
- [x] (No end-user manual — infrastructure slice)

## Ship
- [x] Code review (guidelines §11) — confirmed migration atomicity, FK enforcement, no injection.
      Fixed a **P2/§4 hard-rule violation** (email address was echoed in `InvalidEmail` →
      removed; now a unit variant + test asserts no PII). Added review-suggested integrity wins:
      a **SchemaTooNew** future-version guard and a **file round-trip** test (real reopen/skip
      path). Mutants improved to 14 caught / 6 unviable / 0 survived.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)
