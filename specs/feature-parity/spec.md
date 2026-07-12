# Feature parity — restore what the M9 rewrite dropped

**Constitution:** P8 (spec-driven), P3 (calm + fast), P2 (privacy). **Why:** the M9 Tauri + Leptos
rewrite and the "Soft daylight" design overhaul prioritised correct mail rendering and the new design,
and in doing so did not reimplement several features the Slint build (the v0.1.0 milestone) already
had. v0.1.1 is a big step up in rendering and design but a **functional regression** in these areas.
This effort restores them, mapped onto the current Tauri UI.

## The gaps (restore, in rough value/effort order)

1. **Star / flag** — the `set_star` IPC + `store.set_flagged` + `Message.flagged` already exist; only
   the UI was dropped. (This slice.)
2. **Esc closes search** — the search box has no keyboard close today. (This slice — a tiny fix.)
3. **Save attachments to disk** — the reading pane shows no attachments and can't save them.
4. **Empty trash / permanent delete.**
5. **Drafts** — save & resume.
6. **Folder management** — create / rename / delete.
7. **Smaller:** Markdown compose, address autocomplete, multi-select bulk actions, save/open `.eml`.

Each is its own slice (one branch, one PR), built per `guidelines.md` §11 (tests, gates, user +
technical manual, the review panel). Manuals are updated *back* to include each feature as it returns.

## Slice 1 — Star + Esc-closes-search

**Star.** A **Star** toggle in the reading-pane action row (filled/amber when starred, outline
otherwise) that flips the open message's flag via `api::set_star` — optimistic local update + the
existing server write-back. Starred messages show a small **★** on their list row so they're findable
again. The body DTO doesn't carry the flag, so the reading-pane button uses an `open_flagged` state
captured when the message opens (staying correct even if the message later leaves the list, e.g. after
clearing a search); the list-row ★ reads it from the loaded list.

**Esc closes search.** In the document keydown handler's Escape branch, when the search box is open,
close it and clear the query (re-listing the current folder / merged view). Runs before the typing
guard, so it fires even while the caret is in the search field.

### Out of scope (later slices / not v0.1.0 either)
A dedicated "Starred" filter/folder. Star is findable via the row indicator for now.

## Slice 2 — Trash: Empty Trash + Delete forever

The engine already has `run_empty_folder` and `run_delete_permanently` (server side); only the UI +
IPC were dropped. Both are irreversible, so each goes through a **danger confirm dialog** (per the
feedback rules — a dialog, not an undo toast).

- **Empty Trash** — when the selected folder is Trash, an **Empty Trash** action in the list header →
  confirm → `empty_trash(account)` IPC: resolve the account's Trash folder, empty it on the server
  (`run_empty_folder`), and clear the local rows (new `store::delete_folder_messages`), then re-list.
- **Delete forever** — when the open message is *in* Trash, its **Delete** button permanently removes
  it (confirm → `delete_forever(id)`: `run_delete_permanently` by uid + `delete_message` locally)
  instead of moving to Trash (where it already is).

## Slice 3 — Markdown compose + address autocomplete

Both features already have working **engine/store** support (SEND-6, SEND-9) that the M9 UI never
wired through. This slice is the missing plumbing plus two small UI affordances.

- **Markdown compose** — `run_send` already takes a `markdown: bool` and, when set, renders the body
  with `message::render_markdown` into a `multipart/alternative` (text + HTML) message; the IPC layer
  currently hardcodes `false`. Thread a `markdown` flag through `api::send_message` → `send_message`
  IPC → `run_send`, and add a **Markdown** toggle in the composer footer (off by default; the plain
  body is always sent as the text/plain part, so a reader without HTML still gets readable text).
- **Address autocomplete** — `store::suggest_addresses(account, prefix, limit)` returns distinct past
  senders matching a prefix (LIKE-escaped, case-insensitive). Expose it as a `suggest_addresses` IPC
  command + `api` binding, and show a suggestion dropdown under the To/Cc input in `recipient_field`.
  A pure `rank_suggestions(candidates, already, limit)` helper in `view.rs` drops addresses already
  chipped on the field (case-insensitive) and caps the list; selection funnels through the existing
  `merge_addrs` commit path. Selection is on `mousedown` (with `preventDefault`) so it beats the
  input's blur-commit.

### Out of scope
A rich-text WYSIWYG editor or a live Markdown preview (the toggle sends Markdown; no in-app render
pane — `pulldown-cmark` lives in the engine, and the UI reaches it only over IPC). Suggestions come
from past **senders** only (what the store indexes today), not a separate contacts store.

## Slice 4 — Drafts (save & resume)

The **local-drafts store layer already exists** (SEND-5): a `draft` table + `save_draft` /
`list_drafts` / `draft_by_id` / `delete_draft` and the `DraftContent`/`DraftRow` structs, all
unit-tested but never wired to IPC or UI. `run_send` already takes a trailing `draft_id: Option<i64>`
and deletes that local draft after a successful send; the IPC call currently passes `None`. This slice
is the missing plumbing plus a Drafts view.

- **Save** — a **Save draft** action in the composer footer upserts the current form via a new
  `save_draft(account, draft_id, ComposeDraft)` IPC (`DraftContent` is a 1:1 field map of
  `ComposeDraft`), records the returned id in a `current_draft_id` signal, and closes the composer.
- **Resume** — a **Drafts** entry in the folder rail switches the message-list pane to a draft list
  (`list_drafts` → a `DraftSummary` DTO: id, recipient, subject, snippet, saved-time). Clicking a
  draft loads it (`load_draft(id)` → `ComposeDraft`) back into the composer with `current_draft_id`
  set, so continuing to edit updates the same row and sending clears it.
- **Delete** — each draft row has a delete affordance (`delete_draft(id)`); sending a resumed draft
  deletes it automatically (the `draft_id` now threads through `send_message` → `run_send`).

Mapping/snippet logic (`ComposeDraft` ↔ `DraftContent`, `DraftSummary` preview) lives in the pure,
unit-tested `dto.rs`.

### Out of scope
Server-backed drafts (IMAP `APPEND` to the Drafts folder with `\Draft`, a `FolderRole::Drafts`, and
server-copy lifecycle) — a later slice. **Attachments on a saved draft** — the `draft_attachment`
table exists, but the composer's attachments are file *paths* while the table stores *bytes*, and the
send path reads attachments from paths; bridging that is its own slice. A saved draft keeps text and
recipients; re-attach files when you resume.

## Slice 5 — Save/open .eml

Restores exporting a message as `.eml` and opening a `.eml` file. Engine core (`export_eml`,
`parse_eml`) + store scaffolding (`SAVED_FOLDER`, prune keeps it) survived M9; only UI + IPC were
dropped. No network. **Save** (reading-pane action) → `save_eml(id)`: `export_eml` bytes + a native
save dialog named `<safe_filename_stem(subject)>.eml`. **Open mail file…** (rail entry) →
`open_eml_file(account)`: `parse_eml` into a local `Saved` folder row, then switch + open.
`safe_filename_stem` is pure/tested in `dto.rs`.

## Slice 6 — Multi-select bulk actions

Restores selecting several messages and acting on all at once (ORG-7, dropped from Slint's
`d387558`). **Pure UI** reusing the per-message commands — no new IPC/engine/store.

- **Select** — a hover-revealed checkbox on each list row toggles its id in a `selected:
  HashSet<i64>` (mirrors the existing `read_now`/`marked_unread` pattern); clicking the rest of the
  row still opens it. A **select-all** box in the bulk bar toggles every listed message (pure
  `all_selected(ids, set)` helper in `view.rs`). Selection clears on folder / account / view / search
  change.
- **Bulk bar** — shown while ≥1 row is selected: "N selected" + **Archive**, **Delete**, **Mark
  unread**, **Clear**.
- **Undo** — Archive/Delete reuse the deferred-commit machinery, generalized from a single
  `PendingMove{id}` to `PendingMove{ids}`: the rows hide, one **"N archived · Undo"** toast shows, and
  the server moves (looping `move_to_role`) run only when the window elapses — so Undo stays a pure
  local cancel, and a failed move re-inserts just the rows that failed. Mark-unread is an immediate
  per-message `set_unread` loop (like the single-message action).

### Out of scope
Shift-click range select and Ctrl/Cmd-click (the old slice shipped without them too); a bulk
**mark-read** (there's no `set_read` command yet — marking read happens on open).
