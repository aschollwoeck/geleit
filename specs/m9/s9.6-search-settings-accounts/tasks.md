# S9.6 — Tasks

Status: **complete** — gates green; add-account live-verified against Dovecot; overlay/search/theme
render in-app.

## Engine
- [x] `run_setup` + `build_settings` + `build_smtp_settings` moved into `sync_actions` (Slint app
      re-exports, unchanged, still builds). Unit tests for the validators relocated.
- [x] **Correct** live setup test (`live_setup_creates_and_syncs_an_account`) — opens the DB via
      `open_store` (SQLCipher). **Passes against Dovecot.** Replaces the Slint copy that had been
      broken since encryption-at-rest (opened the encrypted DB with `Store::open`; `#[ignore]` hid it).

## Shell
- [x] `add_account(form)` (validate + `run_setup` on a worker) · `search(account_id, query)` (FTS5) ·
      `set_theme(theme)`

## Frontend
- [x] Add-account overlay (own document, plain form; `setup_field` helper) — credentials → keychain
- [x] Search box in the list header; empty query returns to the folder; **epoch-guarded**
- [x] Theme toggle (persists to the store) + "＋ Account" in the rail
- [x] Empty state offers **Add account** (not a dead end)

## Gates
- [x] fmt · clippy `-D warnings` · tests · deny · wasm · boundary · Slint app builds (incl. dangerous-tls)
- [x] In-app: the setup overlay renders (all fields), the search box + rail tools render
- [ ] Code review agent → then merge

## Notes
- `build_settings`/`build_smtp_settings` are unit-tested but not *mutation*-tested — they live in the
  mutants-excluded `sync_actions.rs` (network-glue file), exactly as when they were in the excluded
  `refresh.rs`. No coverage regression; the unit tests cover defaults / empties / bad ports.
- Deferred (named): multi-account switcher + remove-account UI (`run_remove_account` already moved),
  save/open `.eml`, cross-account search, OAuth (M7). Live search-from-a-click wasn't injection-tested
  (no xdotool); the search command reuses M6-tested FTS5.
