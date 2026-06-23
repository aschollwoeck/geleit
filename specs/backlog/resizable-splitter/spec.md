# Backlog — Resizable list / reading-pane splitter

The message-list ↔ reading-pane boundary was fixed (list 380px). Make it draggable + persisted.

## In scope
- A 6px splitter handle (ew-resize cursor) between the list and reading pane. Drag sets `list-width`
  (clamped 280–680px), computed in a stable window frame (`absolute-position.x + mouse-x`) so it
  doesn't drift as the handle moves. Persisted to the `settings` table on pointer-release; restored
  at startup.
- The native HTML webview tracks the divider: `body_rect` left = rail (240) + `list-width` + handle
  (6), recomputed each frame by the GTK pump — generalizes the previously-hardcoded 620 (=240+380).

## Out of scope
- Resizing the left rail; collapsible panes; a horizontal (top/bottom) split.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. Layout correct at any list width; the webview tracks (verified by launching at a persisted
   non-default width with an HTML message — webview starts right of the wider list, cue still visible).
3. The drag interaction itself (feel/clamps) is the maintainer's eyeball — mouse drags can't be
   injected in this environment.

## Deliverables
- `list-width` prop + splitter handle + `splitter-released` persist; `body_rect` tracks; startup load.
