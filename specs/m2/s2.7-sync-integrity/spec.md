# S2.7 — Sync integrity (property tests) · Spec (the WHAT)

Slice of **M2**. Type: testing/hardening. Strengthens the constitution's core promise — **P6, the
integrity of mail is sacred** — by **proving** (over thousands of random inputs) that the sync
reconciliation never loses or duplicates mail, is idempotent, and resumes correctly after an
interruption.

Status: **draft.**

## Purpose
Add property-based tests (`proptest`) over `reconcile` (the heart of incremental sync + backfill)
that assert the invariants a careful reviewer would want guaranteed, not just spot-checked.

## In scope
- `proptest` as a dev-dependency of `geleit-engine`.
- Property tests for `sync::reconcile(local, server)`:
  - **set identities:** `new = server − local`, `deleted = local − server`, and `new ∩ deleted = ∅`.
  - **no loss / no dupes (convergence):** applying the plan to the local set —
    `(local − deleted) ∪ new` — equals the server set exactly.
  - **idempotent:** reconciling again after applying yields an empty plan.
  - **resumable:** reconcile is a pure function of current state, so from *any* partial-progress
    local set it still converges to the server set (a property over arbitrary `local`).
  - robust to **duplicate** UIDs in the input slices (sets, not sequences).

## Out of scope
- No production-code change (the store's no-dupe `ON CONFLICT` and delete-by-uid are already
  unit-tested; this proves the *planning* logic). Encryption (S2.2). Gmail (S2.5). Status UI (S2.6).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green (proptest licenses pass).
2. The property tests pass (default proptest case count) and genuinely exercise the invariants
   above; a deliberately wrong `reconcile` would fail them (sanity: invariants are non-trivial).
3. `cargo mutants` unaffected — `reconcile` stays mutation-tested; 0 missed.

## Deliverables
- `proptest` dev-dep + property tests in `geleit-engine::sync`. *(No new ADR, no manual.)*
