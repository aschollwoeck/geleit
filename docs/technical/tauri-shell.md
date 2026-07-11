# The Tauri shell + Leptos frontend (M9)

How the new UI is put together, and the handful of things that will bite you if you don't know them.
Decision and evidence: [ADR-0012](../adr/0012-tauri-shell-with-leptos-ui.md),
[the webview spike](tauri-webkit-spike.md).

## Shape

```
crates/geleit-shell   Tauri host: the window, the OS webview, the IPC seam.        (host binary)
crates/geleit-ui      Leptos frontend: components + pure view logic.               (wasm32 + host)
```

`geleit-app` (Slint) still exists and still builds; it is deleted in **S9.7**, at which point
`geleit-shell` takes over its name. Every slice leaves the project working.

`geleit-{core,platform,store,engine}` are untouched.

## Two boundaries, both machine-checked

`scripts/check-boundary.sh` (run in CI) enforces:

1. No engine crate depends on a UI crate (ADR-0003).
2. **`geleit-ui` depends on none of our crates at all.** It reaches the engine *only* over the IPC
   seam. This is the one that actually bites: nothing in Cargo stops a Leptos component from
   `use geleit_store::…` and querying SQLite straight from view code, and the moment one does, the
   seam is decorative. So it is asserted, not hoped for.

## The IPC seam

`geleit-shell/src/ipc.rs` holds the commands; `dto.rs` holds the types and the pure mapping.

- **DTOs, not store types.** The frontend never sees `geleit_store` types, so the schema can evolve
  without breaking the UI.
- **Every command is `async` and hops to a blocking thread** (`spawn_blocking`). SQLite calls block;
  the webview's event loop must not (P1).
- `Store` is not `Sync`, so app state holds only `db_path + Arc<dyn SecretStore>` and each command
  opens the store. SQLCipher open is ~a millisecond — nothing next to the ~630 ms the webview spends
  booting — and it keeps the commands independent and thread-safe. If it ever shows up in a profile,
  a thread-local or a connection pool is the fix; do not reach for a global `Mutex<Store>`, which
  would serialize the UI behind the slowest query.
- `open_message` already carries `html` (unrendered). S9.2 adds the iframe **without changing the
  seam**.

## Frontend build — no npm, no bundler, no Node

```
scripts/build-ui.sh [--release]
  cargo build -p geleit-ui --target wasm32-unknown-unknown
  wasm-bindgen --target web  →  crates/geleit-shell/dist/pkg/
```

That is the whole toolchain. It is what keeps `cargo` and `deny.toml` covering the project's
*entire* dependency tree.

**Gotchas, both of which cost me time:**

1. **`wasm-bindgen` the CLI must exactly match `wasm-bindgen` the crate.** A mismatch fails at
   runtime with an opaque error. `build-ui.sh` reads the version out of `Cargo.lock` and refuses to
   run if the CLI disagrees. CI pins the same version. (The crate version is pinned by `js-sys`, so
   the CLI follows the lockfile, not the other way round.)
2. **Tauri embeds `dist/` into the binary at compile time.** Rebuilding the wasm alone changes
   nothing you can see — you must rebuild `geleit-shell` afterwards:
   ```
   ./scripts/build-ui.sh --release && cargo build -p geleit-shell
   ```

## No inline scripts — and you cannot use them

`index.html` loads `/early.js` and `/boot.js` as files. Do not "simplify" these back into inline
`<script>` blocks:

- **Tauri's CSP nonce injection does not reach inline *module* scripts.** An inline
  `<script type="module">` silently never runs — the app sits on its skeleton forever, which looks
  exactly like a hang. This wasted a debugging cycle; the fix is to keep scripts external.
- External files also let the CSP stay at a strict `script-src 'self'` with no `'unsafe-inline'`.

## Skeleton paint (constitution P3/P4 — a requirement, not polish)

WebKit spends **~630 ms** spawning its web process before a single line of our code runs. So
`index.html` paints the three-pane chrome as static HTML the moment the document parses, and Leptos
replaces `#app` when it mounts. A blank window for two thirds of a second reads as a broken app.

`boot.js` also renders the failure: if the wasm can't load, the user gets a message, not a frozen
skeleton.

## Theme

The **store** is the source of truth (the same `setting` row the Slint app writes), so a user's
choice survives the migration. But `index.html` cannot await IPC and still paint instantly, so it
paints an *optimistic* theme from `localStorage` (falling back to `prefers-color-scheme`), and the
app reconciles against the store on mount, refreshing `localStorage` for next launch.

## Security posture (set here; relied on by S9.2)

- CSP forbids every remote origin: `default-src 'self'; … img-src 'self' data:; frame-src 'none'`.
- `'wasm-unsafe-eval'` is needed to instantiate *our own* wasm. It does not permit `eval` of remote
  script, and mail never runs in this document.
- Webview network context is **`incognito: true`** — no cookie jar, no persistent cache — so image
  loads (once S9.2 allows them on request) cannot be correlated across sessions.
- No Tauri plugins are enabled. There is no filesystem, shell, or HTTP capability to grant.

## Testing

The frontend is split so that the parts worth testing *are* testable without a browser:

| | |
|---|---|
| `geleit-ui/src/view.rs` | Pure display logic (dates, elision). Unit + **mutation** tested on host. |
| `geleit-shell/src/dto.rs` | Pure store→UI mapping, folder ordering. Unit + **mutation** tested. |
| `app.rs`, `api.rs`, `ipc.rs` | View declaration and glue — excluded from mutants (survivors there are spurious), the same split as `geleit-app`'s `main.rs`/`viewmodel.rs`. |

`geleit-ui` compiles for the **host** as well as wasm (`crate-type = ["cdylib", "rlib"]`, wasm
entrypoint behind `cfg(target_arch = "wasm32")`) — that is what lets clippy and `cargo test` cover it
like any other crate. CI *also* builds it for wasm, so a wasm-only break can't slip through.

## Screenshot verification

The build environment can't inject clicks (no `xdotool`), so a **debug-only** seam exists:

```
GELEIT_DB=/path/demo.db GELEIT_OPEN=<message id> ./target/debug/geleit-shell
```

`GELEIT_OPEN` makes the UI open that message on boot, so the reading pane can be screenshot-verified.
It is compiled out of release builds — `dev_open_message` returns `None` and the env var is never
read. S9.2 depends on this: its whole job is rendered mail, which must be verified visually.
