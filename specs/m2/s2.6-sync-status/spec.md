# S2.6 — Non-blocking sync status · Spec (the WHAT)

Slice of **M2**. Type: UI. Delivers **SYNC-4**: a calm, non-blocking indication of what sync is
doing — distinct from errors. Fixes a real wrinkle from S2.4: backfill progress currently appears
in the **danger/error banner**, reading as if progress were a problem.

Status: **draft.**

## Purpose
While a refresh runs (which never blocks the UI — P1), show a quiet status line with the current
phase — "Checking for new mail…", then "Catching up… N" during backfill — that clears when done.
Errors keep their own danger banner; progress is calm (design.md §10).

## In scope
- A new `sync-status` UI property + a **calm** status strip (muted text + a small accent marker on
  `surface`), shown in the message-list header area while syncing.
- Refresh worker phases drive it: "Checking for new mail…" (incremental) → "Catching up… N"
  (backfill) → cleared on completion. Move the "Catching up…" text off the danger `status` banner.
- `status` (danger banner) is reserved for **errors** only; `sync-status` is cleared on error too.

## Out of scope
- A spinner/animation (kept static + calm; the changing count conveys progress). A per-account or
  per-folder breakdown; cancel button; last-synced timestamp. Gmail (S2.5).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. Refresh shows a **calm** sync-status (not the danger banner): "Checking for new mail…" then
   "Catching up… N", cleared when done. Errors still use the danger `status` banner; the two never
   collide. (Verified by launching + reading the wiring.)
3. The UI stays responsive throughout (P1 — unchanged; sync is on the worker).
4. `cargo mutants` unaffected (UI plumbing; `main.rs` excluded) — existing 0-missed holds.

## Deliverables
- `sync-status` property + calm strip + worker wiring. *(No new ADR; brief manual touch.)*
