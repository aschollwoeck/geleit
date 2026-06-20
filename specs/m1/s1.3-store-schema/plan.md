# S1.3 — Local store schema · Plan (the HOW)

Implements `spec.md`. References constitution P1/P6, ADR-0003/0004; produces ADR-0005.

## Crate
`crates/geleit-store` (UI-agnostic). Deps: `rusqlite` (feature `bundled` — compiles SQLite in,
no system dep, cross-platform), `thiserror`, `geleit-core`.

## Schema (migration 1)
Account-scoped throughout (multi-account is later but the schema assumes it now):
```sql
PRAGMA foreign_keys = ON;  -- set per connection on open

CREATE TABLE account (
  id INTEGER PRIMARY KEY,
  email TEXT NOT NULL UNIQUE,
  display_name TEXT,
  created_at INTEGER NOT NULL          -- unix seconds
);
CREATE TABLE folder (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  name TEXT NOT NULL,                  -- server mailbox name
  UNIQUE(account_id, name)
);
CREATE TABLE message (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  folder_id  INTEGER NOT NULL REFERENCES folder(id)  ON DELETE CASCADE,
  uid INTEGER,                         -- IMAP UID within folder
  message_id TEXT,                     -- RFC822 Message-ID
  subject TEXT, from_name TEXT, from_addr TEXT,
  date INTEGER,                        -- unix seconds
  seen INTEGER NOT NULL DEFAULT 0,
  flagged INTEGER NOT NULL DEFAULT 0,
  has_attachments INTEGER NOT NULL DEFAULT 0,
  snippet TEXT,
  UNIQUE(account_id, folder_id, uid)
);
CREATE TABLE body (
  message_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
  plain TEXT, html TEXT
);
CREATE INDEX message_folder_date ON message(folder_id, date DESC);  -- newest-first reads (P1)
```

## Migration runner
Hand-rolled (no extra dep): read `PRAGMA user_version`; for each migration with index ≥ current,
run its SQL in a transaction and bump `user_version`. `MIGRATIONS: &[&str]` is an ordered list;
appending a new entry is how the schema evolves. Encryption (M2) will wrap the connection open,
not the schema, so this stays stable.

## API (this slice)
- `Store::open(path)` / `Store::open_in_memory()` → open connection, `PRAGMA foreign_keys=ON`,
  run migrations.
- `StoreError` via `thiserror`, wrapping `rusqlite::Error` (don't leak it across the API —
  guidelines §4).
- Minimal ops to exercise the schema: `add_account`, `account_by_email`, `list_accounts`,
  `add_folder`, `folders_for_account`. (Message/body CRUD lands with sync, S1.5/S1.6.)

## Wiring
- Add `geleit-store` to workspace `members` (done) and to `scripts/check-boundary.sh`
  `ENGINE_CRATES` (must not depend on the UI crate) and CI `mutants-diff` package list.
- Engine does not depend on store yet (wired when sync needs it, S1.5) — keep this slice scoped.

## Tests (acceptance)
- Migrations create all four tables (query `sqlite_master`); `user_version` = latest; second open
  is idempotent.
- account insert/get/list; duplicate email → UNIQUE error; folder UNIQUE(account,name); deleting
  an account **cascades** to its folders (FK on).

## Docs
- ADR-0005: schema shape, account-scoping, hand-rolled `user_version` migrations, bundled SQLite,
  encryption deferred to M2. Update `docs/technical/workspace.md` crate list.

## Verify
`cargo build/test --workspace`, `clippy -D warnings`, `fmt --check`, `cargo deny check`,
`cargo mutants --package geleit-store` — all green before PR.
