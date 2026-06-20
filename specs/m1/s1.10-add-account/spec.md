# S1.10 — Add account (manual IMAP) · Spec (the WHAT)

Slice of **M1** (addendum — completes the in-app side of **ACC-3**, "connect an account via manual
config", which M1 shipped only via env/tests). Type: UI + store. Makes the app **self-service**: a
person can add their IMAP account and see their mail without env vars.

Status: **draft.**

## Purpose
On first launch (no account) show an **Add account** form (email, IMAP server, port, username,
password). Connecting creates the account, saves its IMAP settings, fetches the inbox, and shows
the mail. The form doubles as **reconnect** (re-enter the password after a restart, since the
keychain is still session-only).

## In scope
- `geleit-store`: persist per-account IMAP settings (migration #2: `imap_host/port/username/
  allow_invalid`); `add_imap_account`, `update_imap_settings`, `imap_settings`, `delete_account`.
- A **session-shared** secret store (`Arc<InMemorySecretStore>`) so the password set at setup is
  available to refresh within the run.
- `geleit-app`: an Add-account form (std-widgets `LineEdit`s) shown when there's no account (or for
  reconnect); `run_setup` (create/update account + store password + first sync, off-thread, rolls
  back a half-created account on failure); `run_refresh` now reads connection settings from the
  **store** (no env needed for normal use). Dynamic folder load after setup.

## Out of scope
- OAuth + provider auto-config (M7). **Persisting the password across restarts** — still the
  in-memory keychain; real OS keychain is SEC-2 (M2). Editing/removing accounts via UI; multi-account.

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. Store: migration #2 applies on an existing db; `add_imap_account`/`imap_settings`/
   `update_imap_settings`/`delete_account` correct (tested).
3. `build_settings` validates email/host/user/port (tested).
4. **Live (`--features dangerous-tls`):** `run_setup` against Dovecot creates the account + syncs
   INBOX; `run_refresh` then reads settings from the store and re-syncs. (Form click is manual.)
5. P1: setup + refresh run off the UI thread; no network on the UI thread. No password in any
   log/error (P2).
6. `cargo mutants` — store additions covered; `refresh.rs` (network glue) excluded; 0 missed.

## Deliverables
- store IMAP-settings + account methods; `geleit-app` Add-account form + `run_setup`;
  `docs/manual/` "Add your account"; roadmap note. *(No new ADR.)*
