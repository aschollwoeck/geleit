# S5.5 — Create / rename / delete folders (ORG-6) · Spec

Slice of **M5**. Manage the server's folder list from the app.

## In scope
- Engine: `imap::create_folder` / `rename_folder` / `delete_folder`. `persist_folders` now **prunes**
  local folders absent from the server list (so rename/delete reconcile, not just add).
- Store: `prune_folders(account, keep)` (remove folders not in `keep`; messages cascade) + test.
- App: a "Manage folders…" rail link → an overlay with a name field + **Create**, and per-folder
  **Rename→** (to the field's text) + **Delete**; each runs on a worker, then reloads the rail.

## Out of scope
- Nested/hierarchical folder creation UI (the server name may contain a delimiter, but no tree UI);
  confirmation on delete; moving folders.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. `prune_folders` removes absent + keeps listed — tested.
3. App: Create/Rename/Delete hit the server then the rail reflects it; failure → calm note
   (maintainer eyeballs; engine folder ops are live-tested glue).
4. `cargo mutants` — store 0-missed.

## Deliverables
- Engine folder ops + prune reconcile; `prune_folders` + test; `run_create/rename/delete_folder`;
  Manage-folders overlay + handlers.
