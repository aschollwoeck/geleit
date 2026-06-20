# S1.9 — Manual refresh · Plan (the HOW)

Implements `spec.md`. The hard part is running the async IMAP sync without blocking the Slint UI
thread, and posting results back safely (nothing `!Send` crosses threads).

## geleit-engine
- `pub fn store_password(secrets: &dyn SecretStore, username: &str, password: &[u8]) -> Result<(), ImapError>`
  — sets the password under the (private) IMAP service key, so the app needn't know `SECRET_SERVICE`.

## geleit-app::refresh (network/glue — excluded from mutants like imap.rs)
- `build_imap_config(host, port, username, allow_invalid_certs) -> Result<ImapConfig, String>`:
  trim; reject empty host/username; parse port (1..=65535). **Pure → unit-tested.**
- `config_from_env() -> Result<(ImapConfig, String /*password*/), String>`: reads `GELEIT_IMAP_HOST`,
  `GELEIT_IMAP_PORT` (default 993), `GELEIT_IMAP_USER`, `GELEIT_IMAP_PASSWORD`, `GELEIT_IMAP_INSECURE`.
- `run_refresh(db_path, config, password) -> Result<(), String>`: open a **worker** `Store`; first
  account or `Err("No account configured yet.")`; `store_password`; a current-thread tokio runtime
  `block_on(sync_folders → sync_envelopes("INBOX",200) → sync_bodies("INBOX",200))`; map any
  `ImapError` to a calm, PII-free message ("Couldn't refresh — check your connection and try again.").

## geleit-app UI (`main.rs`)
- Properties: `in property <bool> refreshing;`, `in property <string> status;` (non-empty = error).
  Tokens: add `danger-strong`/`danger-quiet`. Callback `refresh()`.
- List header: title + a Refresh button (text "Refresh"/"Refreshing…", `enabled: !refreshing`).
- Under the header: `if status != "" :` a danger-quiet banner with a danger-strong guide edge, body
  text in `text` (AA on tint, per design.md §10).
- Wiring (`on_refresh`): ignore if already refreshing; set `refreshing=true`, `status=""`; read
  `config_from_env()`; **spawn a thread**: `run_refresh(...)`, then `slint::invoke_from_event_loop`
  with a **Send** closure capturing `weak`, `db_path` (String), `folder_ids` (cloned `Vec<i64>`),
  and the result → on Ok reload the current folder's messages (open a short-lived store, rebuild the
  model) + `refreshing=false`, `status=""`; on Err set `refreshing=false`, `status=msg`.
  Nothing `!Send` (the `Rc<Store>`/`VecModel`) is captured by the worker or the posted closure.

## Tests
- `build_imap_config`: ok; empty host/user; bad/zero/out-of-range port.
- Live (`#[ignore]`-style, manual): append to Dovecot, Refresh, assert the row appears (run by hand
  against the dev server with the env set + `--features dangerous-tls`).

## Verify
`cargo build/test --workspace`, `clippy -D warnings`, `fmt`, `cargo deny check`, `cargo mutants`,
plus a live refresh against Dovecot. `.cargo/mutants.toml` excludes `geleit-app/src/refresh.rs`.
