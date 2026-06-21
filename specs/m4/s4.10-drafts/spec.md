# S4.10 — Drafts: save & resume (SEND-5) · Spec (the WHAT)

Slice of **M4 (Send)**. Type: store + UI. Save an unfinished message as a local draft and resume it
later; sending a resumed draft removes it.

Status: **draft.**

## In scope
- Store: migration #9 (`draft` table), `DraftContent`/`DraftRow`, `save_draft` (insert/update),
  `list_drafts`, `draft_by_id`, `delete_draft` — round-trip tested.
- App: **Save draft** in the compose overlay; a **Drafts** rail button → a list overlay → click a
  draft to **resume** it in compose (threading preserved). Sending a resumed draft deletes it
  (best-effort, in `run_send`).

## Out of scope
- Auto-save on close/typing (explicit Save only). Server Drafts-folder sync (local-only). Cross-device.

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings` (incl. `--features dangerous-tls`)/`fmt`/`cargo deny check` green.
2. Draft save→list→resume→update(in place)→delete round-trips, incl. references split/join — tested.
3. App: Save draft persists current compose; Drafts list resumes (To/Cc/Subject/Body + threading);
   sending a resumed draft removes it (maintainer eyeballs).
4. `cargo mutants` — store 0-missed.

## Deliverables
- Migration + draft CRUD + test; compose Save-draft + Drafts overlay + resume; `run_send` deletes the
  sent draft.
