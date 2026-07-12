# Tasks — "Soft daylight" desktop reskin

- [x] Tokens (light + dark) → CSS custom properties in `dist/style.css`.
- [x] Bundle Hanken Grotesk + IBM Plex Mono locally (`dist/fonts/`, `fonts.css`, `index.html` link).
- [x] `icons.rs` — `icon()` helper + inline-SVG line set + `folder_icon()`.
- [x] Store: `folder_unread_counts`; `FolderDto.unread`; `list_folders` folds counts.
- [x] IPC: `remove_account`, `get_bool_setting`, `set_bool_setting`, `get_signature`, `set_signature`;
      registered in both invoke handlers; mirrored in `api.rs`.
- [x] Rail: expand ⇄ collapse; account switcher menu; Compose (indigo); folder icons + unread counts +
      guide edge; theme toggle + Settings.
- [x] Message list: header (title · unread · search/refresh); day-grouped rows; row style with account
      dot + guide edge; search field; refresh/catch-up strip; empty states.
- [x] Reading pane: warm surface + guide edge; header; bordered action row; remote-content cue +
      "Load images"; HTML → sandboxed iframe, plain → themed `<pre>`.
- [x] Compose modal (From / To / Cc / Subject / body / Send + draft status).
- [x] Settings window: 5 tabs; theme; signature; remove-account (→ danger confirm); block-remote,
      mark-read, notify toggles (persisted).
- [x] Add-account wizard: provider grid (pre-fills servers) + manual IMAP (wired) + honest OAuth note.
- [x] Toast + auto-dismiss; error toast; confirm dialog.
- [x] Keyboard shortcuts: `c` `/` `e` `#` `r` `f` `Esc`.
- [x] Compiles host + wasm; `build-ui.sh` bundles.
- [x] Screenshot-verify each view (light + dark).
- [x] Gates: fmt, clippy `-D warnings`, tests, `cargo deny`, boundary, mutants.
- [x] Real-mail render re-check (gitignored `.eml`) in the sandboxed iframe.

## Deferred (see plan.md "Deltas")

- [ ] To/Cc recipient chips.
- [ ] Toast **Undo** for a committed archive/delete (needs restore-to-folder).
- [ ] `j`/`k` list navigation; `z` undo.
- [ ] Compose attach / "Aa" / discard buttons.
- [ ] True unified "All accounts" inbox (backend cross-account query).
- [ ] Real OAuth (M7).
