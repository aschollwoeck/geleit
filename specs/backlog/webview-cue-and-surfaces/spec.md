# Backlog — Webview cue visibility + visual pass over remaining surfaces

Reported: the "load further HTML content" affordance sometimes disappears behind the rendered HTML.

## Bug + fix
The HTML viewer is a native X11 child window drawn over the reading-pane body. The webview is placed
at a **fixed** vertical offset that assumed a one-line subject; a subject that **wrapped** pushed the
real header (incl. the "Remote content blocked / Load remote images" cue and the action row) down,
so the webview covered them. (A geometry-callback approach was tried but a wry/X11 child-window
offset made it unreliable.) Fix: **elide the reading-pane subject to one line** so the header is a
fixed height — the fixed offset then always clears the actions + cue. Verified by screenshot with a
long-subject HTML message: cue + actions are fully visible above the webview.

## Visual pass (screenshots, no issues found)
Captured drafts, manage-folders, and the multi-select bar via the `GELEIT_SHOT` hook (added a
`select` state; seeded demo drafts + an HTML newsletter). All render correctly. Regenerated
reading (now HTML + cue) and inbox light/dark.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. With a long-subject HTML message, the cue + actions are not covered by the webview (verified).

## Deliverables
- subject elide + cue reservation in `body_rect`; `GELEIT_SHOT=select`; seed drafts + HTML message;
  screenshots (drafts, manage-folders, multi-select, regenerated reading/inbox) + manual embeds.
