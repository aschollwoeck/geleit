# S9.7 — Tasks

Status: **complete** — Slint + Blitz + ureq gone; gates green; the real `.eml` re-verified.

- [x] Deleted the Slint `crates/geleit-app` (main.rs, htmlrender.rs/Blitz, remoteimg.rs/ureq, viewmodel.rs, refresh.rs)
- [x] Renamed `crates/geleit-shell` → `crates/geleit-app` (crate name, workspace, build-ui.sh,
      check-boundary.sh, mutants.toml, .gitignore) — binary/`.desktop`/packaging keep the name
- [x] `safehtml.rs`: deleted `document()` + `add_font_fallbacks` + `GENERIC_FAMILIES` +
      `font_value_has_generic` and their tests. **`webview_document` is the only wrapper.**
- [x] `deny.toml`: **banned `ureq`**; dropped the Slint royalty-free license allowance; dropped the
      quick-xml + ttf-parser advisory ignores (Slint/Blitz-only, "not encountered"); refreshed the gtk3 note
- [x] `guidelines.md` §13 rewritten for Tauri + Leptos (was Slint)
- [x] **S9.7-2 verified:** `cargo tree` shows no `slint` / `blitz-*` / `ureq` / `anyrender` / `peniko`
      (`wry` remains — it *is* Tauri's OS-webview layer)
- [x] **S9.7-4 verified:** the real TF Bank `.eml` still renders correctly **with zero workarounds** —
      decoded subject, digits, logo, hero image, no black borders. Removing the Blitz hacks broke nothing.

## Gates
- [x] fmt · clippy `-D warnings` · tests · `cargo deny` (ureq banned; advisories/licenses clean) ·
      wasm · boundary · geleit-app (Tauri) builds
- [ ] Code review agent → then merge

## The payoff (ADR-0012 promises now real)
- **No HTTP client at all** — `ureq` banned; "Load images" is a CSP relaxation the webview honours.
- **No pre-alpha renderer** — Blitz gone; a hardened engine renders mail.
- **No dangerous workaround** — `border-collapse:separate!important` (which would corrupt legitimately
  collapsed tables) deleted.

## Out of scope
CI perf budgets → S9.8. Cross-platform packaging stays M8's Linux-only + the softened notes.
