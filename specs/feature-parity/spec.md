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
