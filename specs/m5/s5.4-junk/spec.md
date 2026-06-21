# S5.4 — Junk / Spam (ORG-5) · Spec

Slice of **M5**. The provider's Junk/Spam folder is visible (it's already in the synced folder list),
and a message can be moved **to** Junk ("Spam") or **out** of it ("Not spam" → Inbox).

## In scope
- App: `viewing-junk` (set when the selected folder is Junk/Spam); a reading-pane action that reads
  **Spam** (move to Junk) normally and **Not spam** (move to Inbox) when already in Junk — reusing
  `perform_move` + `find_folder` (junk/spam, inbox).

## Out of scope
- Client-side spam filtering / rules (we rely on the server, per ORG-5); training the server filter.

## Acceptance criteria
1. build/test/clippy -D warnings/fmt/`cargo deny check` green.
2. Junk folder appears in the rail (existing folder sync); Spam moves to Junk, Not-spam moves to
   Inbox; missing Junk/Inbox → calm note (maintainer eyeballs; built on tested move + find_folder).

## Deliverables
- `viewing-junk` + Spam/Not-spam action + `on_toggle_junk`.
