# S9.7 — Plan

1. `git rm -r crates/geleit-app` (the Slint app — nothing depends on it).
2. `git mv crates/geleit-shell crates/geleit-app`; rename the crate in its Cargo.toml. Packaging,
   `.desktop`, and the release workflow keep the binary name `geleit-app`.
3. Workspace members: drop `geleit-shell`, keep `geleit-app` (now the Tauri host) + `geleit-ui`.
4. `scripts/build-ui.sh` output path, `check-boundary.sh` crate list, `.cargo/mutants.toml` globs all
   updated for the rename (and the deleted Slint files dropped).
5. `safehtml.rs`: delete `document()`, `add_font_fallbacks`, `GENERIC_FAMILIES`, `font_value_has_generic`
   and their tests. `webview_document` is the only wrapper.
6. `deny.toml`: ban `ureq`; drop the Slint royalty-free license allowance; drop the quick-xml/ttf-parser
   advisory ignores (Slint/Blitz-only — verified "not encountered"); refresh the gtk3 comment.
7. `guidelines.md` §13 rewritten for Tauri + Leptos; the ADR statuses were already set in the M9
   decision PR (#105).

Verify: `cargo tree` shows no slint/blitz/ureq/anyrender/peniko; all gates green; the real `.eml`
still renders correctly (re-screenshotted).
