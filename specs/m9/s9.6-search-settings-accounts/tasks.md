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
- [x] Code review agent — credential handling confirmed solid; 6 findings fixed (below)

## Code review — findings acted on
Credentials confirmed solid (masked, never logged, form → keychain, `run_setup` errors PII-free,
`allow_invalid_certs` defaults false, search SQL parameterized). Six fixes:

| # | Finding | Fix |
|---|---|---|
| 1 | **STARTTLS defeated by the pre-filled 465 port (med):** checking STARTTLS while the port still read the implicit-TLS default sent STARTTLS to 465 → setup fails. | The checkbox now moves the port to the mode's standard (587↔465) **only** when it's still on the other mode's default — never stomps a typed port. |
| 2 | **A completing refresh/backfill replaced active search results (med):** the re-list paths ignored `query`. | Both re-list paths now re-run the *search* when one is active, else the folder. |
| 3 | **Theme label wrong on a system-dark fresh boot (med):** `dark` init `false` while early.js painted dark from the OS preference. | `dark` seeds from the actually-painted `data-theme`. |
| 4 | **`AccountForm` derived `Debug` over the plaintext password (P2 footgun).** | Hand-written `Debug` redacts the password. |
| 5 | **Stale query left in the box on folder switch (low).** | `choose_folder` clears the query. |
| 7 | **Doc name drift** (`build_imap`/`build_smtp`). | Spec corrected. |

Not fixed (intentional): no `allow_invalid_certs` UI control — the secure default is false, and
self-signed is a dev-only concern (the live test sets it in code).

## Notes
- `build_settings`/`build_smtp_settings` are unit-tested but not *mutation*-tested — they live in the
  mutants-excluded `sync_actions.rs` (network-glue file), exactly as when they were in the excluded
  `refresh.rs`. No coverage regression; the unit tests cover defaults / empties / bad ports.
- Deferred (named): multi-account switcher + remove-account UI (`run_remove_account` already moved),
  save/open `.eml`, cross-account search, OAuth (M7). Live search-from-a-click wasn't injection-tested
  (no xdotool); the search command reuses M6-tested FTS5.
