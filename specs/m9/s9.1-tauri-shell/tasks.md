# S9.1 — Tasks

Status: **complete** — all gates green, verified in-app by screenshot. (Kept current — this is the
hand-off surface, per constitution P8.)

## Engine — shared store bootstrap
- [x] `geleit-engine::localstore` — moved `db_key` + `open_store` out of `geleit-app/src/refresh.rs`
- [x] `refresh.rs` re-exports `open_store` so `geleit-app` (Slint) is unchanged and keeps building
- [x] Engine gains `getrandom`
- [x] Tests: existing key returned · absent → generated + stored · **wrong-size key or read error is
      surfaced, NEVER overwritten**. The app's old `db_key` test was *deleted*, not duplicated: the
      code moved, so its test moved with it — and the engine's version covers the two guards the old
      one didn't.

## `crates/geleit-shell` — Tauri host
- [x] Crate + `tauri.conf.json` (static `frontendDist`, no npm, no `beforeDevCommand`)
- [x] CSP: `default-src 'self'` … no remote origin reachable from the app document
- [x] Webview network context **ephemeral** (`incognito: true` — no cookie jar, no persistent cache)
- [x] State: `db_path` + `Arc<dyn SecretStore>`; commands open the store on a **blocking thread**
- [x] DTOs decoupled from store types (`dto.rs`)
- [x] Commands: `list_accounts` · `list_folders` · `list_messages` · `open_message` · `theme`
- [x] `GELEIT_DB` env override honored (dev bridge, as today)
- [x] App icon (generated from `design.md`'s accent)

## `crates/geleit-ui` — Leptos frontend
- [x] `crate-type = ["cdylib", "rlib"]`; wasm entrypoint behind `cfg(target_arch="wasm32")`, so the
      crate builds on host and the gates cover it like any other crate
- [x] `invoke` shim + `serde-wasm-bindgen` DTO decoding
- [x] Components: folder rail · message list · reading pane · empty states · error toast
- [x] Reading pane shows **plaintext only** (HTML is S9.2)
- [x] Pure view logic (`view.rs`) unit- + mutation-tested on host

## Look & feel
- [x] `design.md` token table → CSS custom properties, light + dark (both screenshot-verified)
- [x] Theme read from the store's `setting` table — **verified the store beats the system preference**,
      so a user's choice survives the migration
- [x] **Skeleton paint**: static three-pane chrome so the window is never blank during WebKit's
      ~630 ms boot (verified: the skeleton is what shows while the wasm loads)
- [x] `boot.js` renders a *failure* too — a frozen skeleton is indistinguishable from a hang

## Build & CI
- [x] `scripts/build-ui.sh` — cargo → wasm32 → `wasm-bindgen --target web` → `dist/pkg/`
- [x] Asserts the wasm-bindgen CLI and crate versions **match** (it caught a real 0.2.125/0.2.126 skew)
- [x] CI: `wasm32-unknown-unknown` target + pinned `wasm-bindgen-cli`, and CI *builds* the wasm so a
      wasm-only break can't slip through
- [x] `check-boundary.sh` extended: engine crates stay UI-agnostic **and `geleit-ui` may not depend on
      any engine crate** — it reaches the engine only over IPC

## Gates (constitution P10)
- [x] `cargo fmt --all --check` · `clippy -D warnings` · `cargo test` · `cargo deny check`
- [x] `cargo mutants` on the new pure logic: **108 caught, 0 missed**
- [x] Launch + **screenshot** verified: folders (Inbox first), messages with unread/star/attachment
      markers, an opened message, both themes, the skeleton
- [x] Code review agent on the diff
- [x] Manual + technical docs updated (`docs/technical/tauri-shell.md`)

## Found and fixed while building this slice
- **`deny.toml` was scanning every platform**, including Android/iOS, where `tauri` declares
  `reqwest` under a mobile-only `cfg`. That tripped the no-egress ban over a crate that can never be
  compiled into a desktop binary. Scoped `[graph] targets` to the platforms we ship — which makes the
  ban *mean* what it claims. If mobile is ever targeted, reqwest's arrival there is now a deliberate,
  reviewed decision.
- **A corrupt `Date:` header could panic the message list.** `civil_from_days` shifted the timestamp
  by 719,468 days; `i64::MIN` overflowed it. Mail carries whatever date the sender wrote, and a
  hostile one is free to be absurd. Now range-guarded (1900–2099) and tested against `i64::MIN`/`MAX`.
  Found by chasing a surviving mutant.

## Explicitly NOT in this slice
HTML rendering / sandboxed iframe (**S9.2**) · virtualization, threading, flags (S9.3) · refresh
(S9.4) · compose (S9.5) · search, settings, accounts (S9.6) · deleting Slint + Blitz (S9.7) · CI perf
budgets (S9.8).
