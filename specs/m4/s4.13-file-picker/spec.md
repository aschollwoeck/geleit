# S4.13 — Native file picker for attachments (SEND-4 finish) · Spec

Slice of **M4**. Replace the type-a-path attachment flow with a real **Browse…** button that opens
the desktop's native file chooser. The manual path field stays as a fallback.

## In scope
- A **Browse…** button in compose; opens the native chooser via **`zenity`** (then `kdialog`) as a
  **separate process** — deliberately NOT an in-process toolkit dialog (`rfd`'s GTK modal loop would
  risk the same GL/GTK clash that caused the earlier crash). On pick, it fills the path field and
  reuses the existing read-and-attach path. Runs on a worker thread; cancel / no-chooser = no-op.

## Out of scope
- Multi-select; drag-and-drop; an in-process portal/`rfd` backend (revisit if a chooser isn't present).

## Acceptance criteria
1. build/test/clippy -D warnings/fmt/`cargo deny check` green (no new dependencies).
2. Browse… opens the chooser; picking a file attaches it; cancel does nothing; absence of a chooser
   leaves the manual path field usable (maintainer eyeballs).
3. P1 honoured: the (blocking) chooser runs on a worker, not the UI thread.

## Deliverables
- `pick_file_via_dialog` (zenity/kdialog subprocess) + Browse… button + worker handler.
