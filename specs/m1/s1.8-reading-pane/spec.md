# S1.8 — Reading pane · Spec (the WHAT)

Slice of **M1** (`roadmap.md`). Type: UI. Delivers **READ-3** (read a message in plain text) and
**READ-7** (mark read/unread — auto on open + manual). Built to `design.md`; store-only (P1).

Status: **draft.**

## Purpose
Click a message → it opens in the reading pane (sender, subject, date, plaintext body from the
local store), with the selected row highlighted (accent-quiet + the guide edge). Opening marks the
message **read** locally; a manual **"Mark as unread"** flips it back. Local only — syncing read
state to the server is M6 (SYNC-5).

## In scope
- `geleit-store::set_seen(message_id, bool)` (local read-state) + test.
- `geleit-app::viewmodel::body_display(Option<&StoredBody>) -> String` (plain; honest placeholder
  for HTML-only / not-yet-downloaded) + tests.
- Slint: message selection (carry the message via the callback), selected-row treatment (the guide
  edge from `design.md`), reading-pane content (subject/sender/date/body), a "Mark as unread"
  action. On open: mark read + refresh the list (unread dot clears).

## Out of scope
- Safe **HTML** rendering (M3 — HTML-only messages show an honest placeholder). Writing read-state
  **back to the server** (M6, SYNC-5). Reply/forward/compose (M4/M5). Refresh/sync (S1.9).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. `set_seen` flips local read state (tested); `body_display` returns plain / honest placeholders
   (tested).
3. The app runs: selecting a message shows its plaintext body in the reading pane; opening clears
   its unread dot; "Mark as unread" restores it. (Verified by launch + the logic tests; the design
   selection treatment matches `design.md`.)
4. Store-only UI path (no network).
5. `cargo mutants` covers `set_seen` + `body_display` (main.rs excluded) — 0 missed.

## Deliverables
- `geleit-store::set_seen`; `geleit-app::viewmodel::body_display`; reading-pane UI + wiring;
  `docs/manual/reading-mail.md` updated. *(No new ADR.)*
