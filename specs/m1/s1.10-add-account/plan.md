# S1.10 — Add account (manual IMAP) · Plan (the HOW)

Implements `spec.md`.

## geleit-store
- Migration **#2** (append-only): `ALTER TABLE account ADD COLUMN imap_host TEXT; ... imap_port
  INTEGER; ... imap_username TEXT; ... imap_allow_invalid INTEGER NOT NULL DEFAULT 0`.
- `struct ImapSettings { host: String, port: u16, username: String, allow_invalid_certs: bool }`.
- `add_imap_account(email, display_name, &ImapSettings) -> Result<i64>` (INSERT incl. imap cols).
- `update_imap_settings(account_id, &ImapSettings)`; `imap_settings(account_id) -> Option<ImapSettings>`
  (None if host is NULL); `delete_account(account_id)` (FK-cascades folders/messages/bodies).
- Tests: migrate an old db; round-trip settings; update; delete cascades.

## geleit-app::refresh (network glue — excluded from mutants)
- `build_settings(email, host, port, username, allow_invalid) -> Result<(String email, ImapSettings), String>`:
  validate email (`geleit_core::looks_like_email`), non-empty host/user, port parse. **Pure → tested.**
- `run_setup(db_path, secrets, email, display_name, settings, password) -> Result<(), String>`:
  open store; `add_imap_account` (UNIQUE email → if exists, `update_imap_settings` = reconnect);
  `store_password` into the **shared** secrets; current-thread tokio `block_on(sync_folders +
  sync_envelopes("INBOX",200) + sync_bodies("INBOX",200))`; on sync error for a *new* account,
  `delete_account` (rollback); calm PII-free messages.
- `run_refresh(db_path, secrets, folder)`: read first account + `imap_settings` → `ImapConfig`;
  sync (password already in shared secrets). No env for the normal path.

## geleit-app (`main.rs`)
- Session `secrets = Arc<InMemorySecretStore>` shared into setup/refresh workers (Arc is Send+Sync).
- Dynamic state: `folders_model: Rc<VecModel<SharedString>>`, `folder_ids: Rc<RefCell<Vec<i64>>>`,
  `messages: Rc<VecModel<MessageItem>>`. `reload_all()` (UI thread) re-reads account/folders/messages
  + sets `needs-setup`.
- UI: `in property <bool> needs-setup;` toggles between the 3-pane and the Add-account card
  (`LineEdit`s two-way-bound to `f-email/f-name/f-host/f-port/f-user/f-pass`, `setup-busy`,
  `setup-error`, callback `connect()`). design.md tokens; the guide edge on the card.
- `on_connect`: validate via `build_settings`; off-thread `run_setup`; on Ok → `reload_all`
  (shows mail); on Err → `setup-error`. `on_refresh`: shared secrets + store settings; if the
  password is missing (post-restart), show the form pre-filled to reconnect.

## Verify
gates; live `run_setup`/`run_refresh` against Dovecot (`--features dangerous-tls`); app launches to
the form on a fresh db. `refresh.rs` stays mutants-excluded.
