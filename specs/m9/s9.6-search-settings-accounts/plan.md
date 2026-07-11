# S9.6 — Plan

## Engine
Move `run_setup` + the pure validators `build_settings`/`build_smtp_settings` from the Slint
`refresh.rs` into `geleit_engine::sync_actions`; re-export (the pattern). Add a **correct** live
`#[ignore]` setup test (opens the DB with `open_store`, SQLCipher-aware) — the old Slint copy had been
broken since encryption-at-rest (it used `Store::open`, unencrypted; `#[ignore]` hid it from CI).

## Shell
- `add_account(form)` — validate (pure) → `run_setup` on a worker (network + keychain).
- `search(account_id, query)` — `store::search_messages` (FTS5, M6); returns list rows.
- `set_theme(theme)` — `store::set_setting("theme", …)`.

## Frontend
- Setup overlay (own document, plain form); `setup_field` helper per field. Credentials → shell →
  keychain; never a webview, never logged.
- Search box in the list header; empty query returns to the folder; epoch-guarded.
- Theme toggle + "＋ Account" in the rail; the empty state offers Add account (not a dead end).

## Tests
`build_settings`/`build_smtp_settings` unit-tested (defaults, empties, bad ports). `run_setup` live
against Dovecot. Search reuses M6-tested FTS5. Frontend verified by screenshot (setup overlay, search
box, rail tools).
