# S1.10 — Add account (manual IMAP) · Tasks

Derived from `spec.md` + `plan.md` (P8). UI + store slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] store: migration #2 (imap_* columns); `ImapSettings`; `add_imap_account`,
      `update_imap_settings`, `imap_settings`, `delete_account` + tests
- [x] refresh: `build_settings` (+tests); `run_setup`; `run_refresh` reads settings from store
- [x] app: session-shared `Arc<InMemorySecretStore>`; dynamic folders (`RefCell`) + `reload_all`
- [x] app UI: Add-account form (LineEdits), `needs-setup` view switch, `connect()`, setup busy/error
- [x] `on_refresh` reconnect path (prefilled form when password missing)
- [x] `docs/manual/` "Add your account"; roadmap note (S1.10 / ACC-3 in-app)

## Verify (acceptance criteria)
- [x] AC1 build/test/clippy/fmt/deny green
- [x] AC2 store migration + IMAP-settings methods tested (round-trip/update/cascade)
- [x] AC3 `build_settings` validation tested (5 cases)
- [x] AC4 LIVE (`--features dangerous-tls`): `run_setup` creates account + syncs INBOX; `run_refresh`
      reads store settings + session password + re-syncs (test passes)
- [x] AC5 P1 off-thread (workers + invoke_from_event_loop, only Send crosses); P2 no password/PII
      in logs/errors; app launches to the form on a fresh db
- [x] AC6 mutants store+app: 41 caught / 9 unviable / 0 missed (refresh.rs+main.rs excluded)

## Ship
- [x] Code review (guidelines §11) — verdict sound (P1 boundary correct, P2 clean, store correct).
      Fixed: account no longer leaks if `store_password` fails (rollback); reconnect keys on the
      single existing account so editing the email can't create a hidden 2nd account; `reload_all`
      clears the stale status banner.
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)