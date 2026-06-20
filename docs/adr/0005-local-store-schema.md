# ADR-0005: Local store schema & migrations

## Status
Accepted (slice S1.3).

## Context
The local SQLite store is the source of truth for the experience (constitution P1), and the
foundation every sync/read slice builds on. We need an account-scoped schema and a way to evolve
it safely. Multi-account is a later milestone, but the schema must assume it from day one
(roadmap), and encryption at rest is M2 (SEC-1).

## Decision
- A UI-agnostic crate **`geleit-store`** using **`rusqlite`** with the **`bundled`** feature
  (SQLite compiled in — no system dependency, consistent across Windows/macOS/Linux).
- **Account-scoped schema** (migration 1): `account`, `folder`, `message` (envelope fields),
  `body` — every row tied to an `account_id`, with `ON DELETE CASCADE` foreign keys and the
  uniqueness/index constraints sync needs (`UNIQUE(account_id, folder, uid)`, a
  `folder + date DESC` index for newest-first reads, P1). Foreign keys are enabled per connection
  (`PRAGMA foreign_keys = ON`).
- **Hand-rolled migration runner** keyed on SQLite's `user_version`: an ordered, **append-only**
  `MIGRATIONS` list; each pending migration runs in its own transaction and bumps `user_version`.
  No external migration crate (keeps the dependency surface small). Released migrations are never
  edited — only appended.
- Errors are a `thiserror` `StoreError` that **wraps** `rusqlite::Error` (guidelines §4 — callers
  don't see the third-party type directly).

## Consequences
- **Encryption at rest (M2)** wraps the *connection open* (e.g. SQLCipher / an encrypted VFS),
  not the schema — so this ADR is unaffected by it; the schema and migration runner stay stable.
- New tables/columns arrive as appended migrations; the `user_version` mechanism makes upgrades
  deterministic and idempotent.
- Message/body read-write lands with the sync slices (S1.5/S1.6); this slice ships the schema
  plus account/folder operations to exercise and verify it.
- `geleit-store` is engine-side (`app → engine → {core, platform, store}`); the boundary check and
  CI mutation testing cover it. rusqlite's transitive `Zlib` license (foldhash) was added to the
  `cargo-deny` allowlist deliberately (permissive, MIT-compatible).
