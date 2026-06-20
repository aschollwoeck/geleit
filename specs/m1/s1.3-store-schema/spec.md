# S1.3 — Local store schema · Spec (the WHAT)

Slice of **M1** (`roadmap.md`). Type: **infrastructure/engine** — no user stories directly;
measurable acceptance, no end-user manual (guidelines §11). Real workspace code. Produces ADR-0005.

Status: **draft.**

## Purpose
The local SQLite store is the source of truth for the experience (constitution P1). This slice
lays its **account-scoped schema + a migration mechanism** — the foundation every sync/read slice
(S1.4–S1.9) builds on. Plain SQLite for now; **encryption at rest is M2** (SEC-1), so the schema/
open path is designed to accept an encrypted backend later without structural change.

## In scope
- A new UI-agnostic crate **`geleit-store`** (rusqlite, bundled SQLite; thiserror; geleit-core).
- **Schema (account-scoped from day one):** `account`, `folder`, `message` (envelope fields),
  `body` — with foreign keys (cascade) and the uniqueness/索引 needed for sync (e.g. UID per
  folder, newest-first index).
- A **migration runner** (SQLite `user_version`, applied in a transaction) so the schema can
  evolve; migration 1 creates the above.
- `Store::open(path)` / `open_in_memory()` that enables foreign keys and runs migrations; plus
  minimal **account + folder** operations to exercise the schema (message/body CRUD arrives with
  the sync slices).
- Wire into workspace; extend the boundary check + CI mutation testing to the new crate.

## Out of scope
- Encryption at rest (M2). Message/body read/write beyond the schema (S1.5/S1.6). IMAP (S1.4).
- An external migration crate (hand-rolled runner keeps deps minimal).

## Acceptance criteria (measurable)
1. `cargo build/test --workspace` green; `clippy -D warnings`, `fmt`, **`cargo deny check`** clean
   (rusqlite + transitive licenses pass the gate).
2. Opening a fresh store runs migrations and creates all tables (verified via `sqlite_master`);
   `user_version` is set; opening again is idempotent (no re-apply).
3. Account-scoping works: insert/query accounts and folders; **UNIQUE** (email; account+folder
   name) enforced; **foreign keys ON** with cascade delete verified.
4. `cargo mutants --package geleit-store` runs and reports.
5. ADR-0005 (store schema & migrations) recorded; workspace doc updated.

## Deliverables
- `crates/geleit-store/` (schema, migration runner, account/folder ops, tests).
- Root `Cargo.toml` member; `scripts/check-boundary.sh`; `.github/workflows/ci.yml` (mutants).
- `docs/adr/0005-local-store-schema.md`; updated `docs/technical/workspace.md`.
- *(No end-user manual — infrastructure slice.)*
