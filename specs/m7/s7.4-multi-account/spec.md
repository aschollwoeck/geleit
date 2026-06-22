# S7.4/S7.5 — Multiple accounts + switcher (ACC-5/6, MULTI-1/2) · Spec

The multi-account part of **M7** (no OAuth — that's S7.1–S7.3, gated on real Gmail/MS credentials).
Several accounts at once, switch between them, correct from-address on send.

## In scope
- Store: `account_by_id`; the schema is already account-scoped (folders/messages/search isolate
  per account). Test proves isolation + lookup.
- Workers (refresh.rs): every per-account worker now takes an explicit `account_id` instead of
  resolving "the first account" — `run_refresh`/`run_backfill`/`run_send`/`run_set_flag`/`run_move`/
  `run_delete_permanently`/`run_empty_folder`/`run_create|rename|delete_folder`/`run_remove_account`.
  `run_setup` keys on **email** (new email → new account; existing → reconfigure) and returns the id.
- App: the `current-account` UI prop is the single source of truth for the account in view.
  `reload_all` keeps the current account if it still exists, else the first, and fills an accounts
  model. A **rail switcher** lists the other accounts (click to switch → reload + refresh) + a
  **"+ Add account"** entry that opens a blank setup form without dropping the current account
  (Cancel to back out). Remove-account removes the *current* account and falls back to another.
- **MULTI-2** falls out: `run_send` uses the in-view account, so replies/new mail get the right
  from-address + signature automatically.

## Out of scope
- OAuth / token refresh (S7.1–S7.3); a unified all-accounts inbox; per-account sync scheduler
  (refresh is still on-demand/per-account); concurrent background sync of non-visible accounts.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. `account_by_id` + per-account isolation (folders/messages/search) tested; store mutants 0-missed.
3. App: add a 2nd account, switch between them, each shows its own mail/folders; sending uses the
   in-view account; removing the current account falls back (maintainer eyeballs).

## Deliverables
- `account_by_id` + test; account_id threaded through all workers; `run_setup` add-or-reconfigure +
  returns id; switcher UI + add/cancel/switch handlers; `current-account` prop wiring.
