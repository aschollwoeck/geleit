# Backlog — Launch-into-state flag + click-only screenshots

To screenshot views behind a click (compose/search/settings/reading) without input injection, add a
dev hook that drives the UI into a state shortly after launch.

## In scope
- App: `GELEIT_SHOT=<state>` env hook — a single-shot timer (~600ms after launch) invokes the
  matching action: `compose`, `search`, `read`, `settings`, `drafts`, `manage-folders`. Inert unless
  set; only triggers actions the user could take. Held alive until `run()`.
- Captured + embedded: compose, search, reading, settings (+ existing setup/inbox/dark).
- Bug fix surfaced by the reading shot: the reading-pane body didn't wrap (a bare `Text` in a
  `ScrollView` had no width) — wrapped it in a viewport-filling layout so it wraps + top-aligns.

## Out of scope
- A general scripting/automation interface; capturing transient toasts.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. The four states render correctly (verified by the captured screenshots); body text wraps.

## Deliverables
- `GELEIT_SHOT` hook; reading-pane wrap fix; compose/search/reading/settings images + manual embeds.
