# S9.7 — Teardown: delete Slint, Blitz, and ureq

**Milestone:** M9. **Decision:** [ADR-0012](../../../docs/adr/0012-tauri-shell-with-leptos-ui.md).

## What it delivers

The migration's payoff: the old stack is **removed**, and the tighter guarantees ADR-0012 promised
become real — no HTTP client at all, no pre-alpha renderer, no dangerous workarounds.

- **Delete the Slint app** (`crates/geleit-app`): `main.rs`, `htmlrender.rs` (Blitz), `remoteimg.rs`,
  `viewmodel.rs`, `refresh.rs` (its re-exports are the only thing left — all logic already lives in
  the engine). Nothing depends on it.
- **Rename `crates/geleit-shell` → `crates/geleit-app`** so the shipped binary, `.desktop`, and
  packaging keep the name `geleit-app` / GeleitMail.
- **Delete the two Blitz workarounds** from `safehtml.rs`: `document()` (unused once Slint is gone)
  and `add_font_fallbacks()` + its helpers — including `border-collapse:separate!important`, which is
  *actively wrong* for a real engine. `webview_document()` (no workarounds) is the only wrapper left.
- **The app now has no HTTP client.** `ureq` and `blitz-*`/`anyrender`/`peniko` leave with the Slint
  crate; `ureq` goes on the `deny.toml` **ban list**.
- **Clean `deny.toml`:** ban `ureq`; drop the Slint royalty-free **license** allowance; drop the
  advisory **ignores** that existed only for Slint/Blitz (verified by `cargo deny` reporting them
  "not encountered").
- **Update the gates & docs:** `check-boundary.sh`, `.cargo/mutants.toml`, CI, `guidelines.md`, and
  the superseded ADRs' status.

## Acceptance

| | |
|---|---|
| **S9.7-1** | The Slint crate is gone; the workspace builds `geleit-app` (Tauri) + `geleit-ui`. |
| **S9.7-2** | `cargo tree` shows **no** `slint`, `blitz-*`, `ureq`, `anyrender`, `peniko`. |
| **S9.7-3** | `safehtml` has no `document()` and no `border-collapse`/font-fallback code; `webview_document` is the only wrapper. |
| **S9.7-4** | All gates green; the app still renders mail correctly (re-verified with the real `.eml`). |

## Out of scope

CI perf budgets (S9.8). Cross-platform packaging (still M8's Linux-only + the softened
macOS/Windows note).
