# S2.7 — Sync integrity (property tests) · Plan (the HOW)

Implements `spec.md`. Test-only; no production change.

## geleit-engine
- `[dev-dependencies] proptest`.
- In `src/sync.rs`, a `#[cfg(test)] proptest!` block over `reconcile`, with inputs
  `local: Vec<u32>`, `server: Vec<u32>` (proptest `vec(any::<u32>(), 0..N)`; duplicates allowed):
  - `new` ⊆ `server`, `new ∩ local = ∅`, and `new` covers every `server∖local`.
  - `deleted` ⊆ `local`, `deleted ∩ server = ∅`, covers every `local∖server`.
  - `new` and `deleted` are disjoint.
  - **Convergence:** `(local_set ∖ deleted) ∪ new == server_set` (as `HashSet`s) — no loss, no extra.
  - **Idempotent / resumable:** let `local2 = (local_set ∖ deleted) ∪ new`; `reconcile(local2,
    server)` returns empty `new` and empty `deleted`.

Keep the existing hand-written unit tests (named edge cases) alongside the properties.

## Verify
`cargo test -p geleit-engine`; gates; `cargo deny check` (proptest + transitive licenses);
`cargo mutants -p geleit-engine` (reconcile still 0-missed).
