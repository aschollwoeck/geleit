# S2.8 — Remove account (wipe) + offline reading · Plan (the HOW)

Implements `spec.md`.

## geleit-engine
- `pub fn delete_password(secrets: &dyn SecretStore, username: &str) -> Result<(), ImapError>`
  → `secrets.delete(SECRET_SERVICE, username)` (mirrors `store_password`/`has_password`).

## geleit-app::refresh
- `run_remove_account(db_path, secrets: &dyn SecretStore) -> Result<(), String>`:
  open store → first account (Ok if none) → if it has `imap_settings`, `delete_password(username)`
  (best-effort) → `store.delete_account(id)` (cascades). Calm PII-free messages. Non-network but
  touches the keychain (D-Bus), so run on a worker thread.
- CI test (`run_remove_account` over a temp-file db + `InMemorySecretStore`): add account+settings,
  store password, a folder+message+body; remove → `list_accounts` empty, password `get` → None,
  `messages_in_folder` empty. (No network, no live keychain.)

## geleit-app (`main.rs`)
- Slint: a `private property <bool> confirm-remove` toggled in-Slint. Rail footer shows
  "Remove account" → sets `confirm-remove`; the confirm row shows the consequence + **Remove**
  (→ `remove-account()` callback, reset flag) / **Cancel** (reset flag). `callback remove-account()`.
- `on_remove_account`: worker thread → `run_remove_account` → post `invoke_reload` (no account →
  Add-account form). Errors → `status`.

## OFF-1 test
- A store/integration test: open store, add account+folder+message+body, then read
  `messages_in_folder` + `body_for` and assert content — pure local reads, demonstrating offline
  reading (the read path never touches the network).

## Verify
gates; the two new deterministic tests; `cargo mutants` (store covered; refresh.rs/imap.rs excluded);
app launches; manual update.
