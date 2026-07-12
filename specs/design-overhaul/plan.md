# Plan — "Soft daylight" desktop reskin

## Approach

Reskin in place. The IPC seam, store, and engine are unchanged; this slice replaces the
*presentation* (`geleit-ui` `view!` + `dist/style.css`) and adds the small backend affordances the
design needs (folder unread counts, boolean settings, per-account signature read, remove-account).

The `.dc.html` prototype's runtime (`support.js`, `<x-import>`, `sc-if`/`sc-for`) is preview-only.
The target is Leptos: prototype logic → signals, DOM → `view!`, inline styles → CSS classes keyed off
design tokens. Mail HTML is never rendered in the app document — it stays in the sandboxed `mail://`
iframe (ADR-0012), unchanged by this slice.

## Pieces

1. **Tokens + fonts.** Full light/dark token set as CSS custom properties in `dist/style.css`
   (`:root` / `:root[data-theme="dark"]`). Hanken Grotesk + IBM Plex Mono subset-bundled into
   `dist/fonts/` (~200 KB) with a local `fonts.css`; `index.html` links it. Never fetched at runtime
   (CSP `font-src 'self'` + local-first).
2. **Icons.** `icons.rs`: an `icon(svg)` helper (`inner_html` span) + the inline-SVG line set as
   `&'static str` consts, reused verbatim from the prototype.
3. **Backend affordances.**
   - `geleit-store::folder_unread_counts` → folded into `list_folders` (`FolderDto.unread`).
   - `get_bool_setting` / `set_bool_setting` (store `setting` k/v: `"1"`/`"0"`).
   - `get_signature` (read; `set_signature`/`update_signature` already existed).
   - `remove_account` command → `engine::sync_actions::run_remove_account`.
   All registered in both invoke handlers; mirrored in `api.rs`.
4. **App shell** (`app.rs`). One `App()` component, one inline `view!`. Signals for data + overlays;
   handlers for folder/message/move/search/compose/settings/wizard; a boot `Effect`; a keyboard
   listener; toast auto-dismiss. Views inline (rail, list, reading, compose, settings, wizard,
   confirm, toasts) so closures capture signals directly rather than threading callbacks.

## Verification

- `cargo check`/`clippy -D warnings` host **and** `wasm32-unknown-unknown`.
- `build-ui.sh` (wasm-bindgen 0.2.125) produces the bundle.
- Screenshot-verified each view in light and dark via a static class-level harness driven by
  headless chromium (rail+list+reading, compose, settings, wizard).
- Real-mail render re-check (the gitignored TF-Bank `.eml`) in the sandboxed iframe.
- Gates: fmt, clippy, tests, `cargo deny`, boundary, mutants.

## Deltas from the spec (honest)

Built as specified except these, deferred to keep the slice bounded — the *look* is complete; the
missing bits are interaction polish, not layout:

- **To/Cc as plain inputs**, not recipient chips. Chips are a visual nicety; the plain field sends
  identically. (`compose-chips` follow-up.)
- **Toast has no Undo for a committed move.** Archive/delete are optimistic + toast, but the server
  move is committed; reversing it needs a restore-to-folder action. Undo is deferred (as the spec's
  own out-of-scope note anticipated).
- **`j`/`k` list navigation and `z` undo** not wired. `c` / `/` / `e` / `#` / `r` / `f` / `Esc` are.
- **Compose footer** is Send + draft status only; attach / "Aa" / discard buttons deferred.
- **Star/flag affordance dropped** — the "Soft daylight" reading pane has no star; the `set_star`
  backend remains, unused by this UI. (Re-surface if a later design wants it.)
