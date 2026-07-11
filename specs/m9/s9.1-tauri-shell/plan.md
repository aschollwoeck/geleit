# S9.1 — Plan

## Crate layout

Two new crates, added **alongside** the still-working Slint app:

| Crate | What | Target |
|---|---|---|
| `crates/geleit-ui` | Leptos CSR frontend — components, view state | `wasm32-unknown-unknown` (cdylib) + host (rlib, so gates/tests run) |
| `crates/geleit-shell` | Tauri host — window, commands, store access | host binary |

`geleit-app` (Slint) is untouched and keeps building. It is deleted in **S9.7**, at which point
`geleit-shell` takes over the `geleit-app` name.

Dependency direction is unchanged (ADR-0003): `shell → engine → {core, platform, store}`; `ui`
depends on **nothing of ours** — it talks only over IPC. That is deliberate: the frontend cannot
reach the store even by accident.

## The IPC seam

Commands are the *only* way the UI touches data. Each is `async` (P1 — never block the UI) and
returns a serde DTO, not a store type — so the store's schema can change without breaking the UI.

```rust
#[tauri::command] async fn list_accounts()                        -> Result<Vec<AccountDto>, String>
#[tauri::command] async fn list_folders(account_id: i64)          -> Result<Vec<FolderDto>, String>
#[tauri::command] async fn list_messages(folder_id: i64, limit)   -> Result<Vec<MessageDto>, String>
#[tauri::command] async fn open_message(id: i64)                  -> Result<MessageBodyDto, String>
```

The `Store` is **not** `Sync`-friendly across threads, and the commands are blocking SQLite calls.
So the shell holds `db_path + Arc<dyn SecretStore>` in Tauri state and each command opens the store
on a **blocking thread** (`tauri::async_runtime::spawn_blocking`). This keeps the webview's event
loop free and mirrors the existing worker-thread discipline in `refresh.rs`.

`open_message` returns the plaintext body **and** the (unrendered) HTML string, so S9.2 only has to
add the iframe — the seam does not change.

## Shared code: move `db_key` / `open_store` into the engine

They currently live in `geleit-app/src/refresh.rs` but are UI-agnostic (keychain → SQLCipher key →
open the encrypted store). Both apps need them, and duplicating them would be a real bug risk — the
`db_key` logic deliberately **never overwrites a present key** (a transient keychain failure must not
brick the DB). Move to `geleit-engine::localstore`; `refresh.rs` re-exports so `geleit-app` is
unchanged. Engine gains `getrandom`.

## Frontend build (no npm, no bundler)

`cargo build --target wasm32-unknown-unknown -p geleit-ui --release` → `wasm-bindgen --target web`
→ `crates/geleit-shell/dist/pkg/`. Driven by `scripts/build-ui.sh`; Tauri's `frontendDist` points at
`dist/`, which also holds the hand-written `index.html` and `style.css`.

`wasm-bindgen-cli` **must match** the `wasm-bindgen` crate version Leptos resolves (0.2.126). Pinned
in the script and in CI (`taiki-e/install-action`), same discipline as the pinned `cargo-deny`.

## Skeleton paint (P3/P4)

`index.html` contains the three-pane chrome as **static HTML+CSS** — rail, list column, reading
pane, with shimmering placeholder rows. It paints the instant WebKit parses the document, ~630 ms
before WASM could possibly mount. Leptos then mounts into `#app` and replaces it. The skeleton uses
the same `design.md` tokens, so the swap is a content change, not a visual jump.

## Theme

`design.md`'s token table → CSS custom properties on `:root`, with a `[data-theme="dark"]` override.
Initial theme read from the store's existing `setting` k/v table (key `theme`), applied by inline
script **before first paint** so there is no light-then-dark flash.

## Security config (set now, relied on in S9.2)

- Tauri CSP: `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'
  'unsafe-inline'; img-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost`.
  No remote origin is reachable from the app document.
- Webview network context: **ephemeral** — no cookie jar, no persistent cache.
- `wasm-unsafe-eval` is required to instantiate *our own* WASM module; it does **not** permit `eval`
  of remote script, and mail HTML never runs in this document anyway (S9.2 confines it to a
  `sandbox`ed iframe with no `allow-scripts`).

## Tests

- **Engine:** `localstore::db_key` — returns an existing key; generates on absent; **refuses to
  overwrite** a wrong-size key or a read error (the brick-the-DB guard). Mutation-tested.
- **Shell:** each command against an in-memory/temp store — folder order (Inbox first), newest-first
  messages, `open_message` returns plain + html, unknown ids error cleanly.
- **UI:** pure view logic (date formatting, sender display-name fallback, snippet elision) as plain
  Rust unit tests on the host target — no DOM needed.
- **Manual/visual:** launch + screenshot (the maintainer-eyeball boundary, as always for UI).

## Risks

| Risk | Mitigation |
|---|---|
| `geleit-ui` must compile for **host** too (so clippy/tests run) | `crate-type = ["cdylib", "rlib"]`; keep `wasm_bindgen(start)` behind `#[cfg(target_arch = "wasm32")]` |
| wasm-bindgen CLI/crate version drift | Pin both; script asserts the versions match and fails loudly |
| CI has no wasm target | Add `wasm32-unknown-unknown` + pinned `wasm-bindgen-cli` to the workflow |
