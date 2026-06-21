# S5.3 — Empty trash / delete permanently (ORG-2) · Spec

Slice of **M5**. In the Trash folder, delete a single message permanently and "Empty Trash".

## In scope
- Engine: `imap::delete_permanently(folder, uid)` (UID STORE `\Deleted` + `UID EXPUNGE`);
  `imap::empty_folder(folder)` (STORE `1:*` `\Deleted` + `EXPUNGE`). (`drain` now pins the stream so
  the !Unpin expunge stream works.)
- App: `viewing-trash` (set when the selected folder is Trash); an **Empty Trash** button (clears the
  local folder + worker `run_empty_folder`); the **Delete** action becomes **permanent** when in
  Trash (optimistic remove + worker `run_delete_permanently`).

## Out of scope
- Per-account "permanently delete" outside Trash; confirmation dialog (it's already 2-step: delete→Trash, then empty).

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. In Trash: Delete removes permanently; Empty Trash clears the folder (optimistic) + expunges on the
   server (maintainer eyeballs; engine ops are live-tested glue).

## Deliverables
- `delete_permanently` + `empty_folder` (+ `drain` pin fix); `run_delete_permanently` +
  `run_empty_folder`; `viewing-trash` + Empty Trash button + permanent-in-Trash Delete.
