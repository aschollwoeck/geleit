# S2.4 — Progressive backfill · Spec (the WHAT)

Slice of **M2**. Type: engine + UI glue. Delivers **SYNC-3**: after the recent window (S2.3), fill
in the **rest of the mailbox** newest-first, in batches, in the background — so the whole account
becomes available offline without making the user wait.

Status: **draft.**

## Purpose
A refresh shows recent mail fast (S2.3), then keeps fetching older messages batch-by-batch until the
folder is fully local — the list grows live and a calm status shows progress; the UI never blocks (P1).

## In scope
- `geleit-engine::imap::backfill_folder(..., batch_size, on_batch)`: fetch all server UIDs missing
  locally, **newest-first**, in `batch_size` chunks (envelopes+bodies per chunk), calling `on_batch`
  with the running count. Resumable (each chunk commits; a restart continues from local state).
- Refactor: extract `fetch_envelopes_for`/`fetch_bodies_for` helpers (shared by incremental + backfill).
- `geleit-app`: after the incremental sync in `run_refresh`, run backfill on the same worker; per
  batch, post a list reload + a "Catching up… N" status; clear it when done. Button stays
  "Refreshing…" until the whole sync (recent + backfill) finishes.

## Out of scope
- A dedicated rich progress UI / cancel button (S2.6 — this slice reuses the `status` line).
  Gmail label specifics (S2.5). Encryption (S2.2, deferred). Multi-folder backfill (INBOX only here).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. **Live (`--features dangerous-tls`):** with several messages present, an incremental sync capped
   to a small limit fetches only the most-recent; `backfill_folder` then fetches the remainder
   (envelopes **and** bodies), newest-first; re-running backfill is a no-op (idempotent).
3. `on_batch` is called with a monotonic running total; backfill of an already-complete folder
   fetches 0.
4. P1/P2 unchanged (runs on the worker; no PII in errors). The UI list grows as batches land.
5. `cargo mutants` — shared/store logic covered; `imap.rs` excluded; 0 missed.

## Deliverables
- `backfill_folder` + extracted fetch helpers; `run_backfill` + app wiring with progress status;
  live test. *(No new ADR.)*
