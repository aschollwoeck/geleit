# Backlog — Fix HTML-message crash (X11 BadValue from a degenerate webview)

Reported: clicking an HTML message shows the black loading screen, then the app crashes.

## Root cause (reproduced)
The HTML viewer is a native X11 child window sized by `body_rect`. Since the splitter landed, its
left edge is `rail(240) + list_width + handle(6)`. On a window narrow enough (or with the list
dragged wide) the reading region collapses, so the webview is given a **zero/negative size** →
`set_bounds` issues an X request with an out-of-range integer → **X `BadValue`** → async X error →
abort. Reproduced: 700px window + 680px list + open HTML → `BadValue`, dies in ~2s. (Not the old GL
crash — this build has no GL renderer.)

## Fix
- Clamp the body region's left edge to keep a **minimum reading width** (`MIN_READING_W = 80`), so the
  webview never gets a degenerate size; the same clamp is applied to the list element in the UI, so the
  webview stays aligned with the list edge.
- Guard `show_html` + the reposition pump with `body_too_small`: if the region is still too small
  (extremely narrow window), **hide** the webview (the plain-text fallback shows) rather than hand
  webkit a degenerate surface.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. The repro (narrow window + wide list + open HTML) no longer crashes; HTML still renders on a normal
   window. (Verified: old binary dies with BadValue; hardened binary survives both narrow + wide.)

## Deliverables
- `body_geom`/`body_too_small` + min-reading clamp; UI list-width clamp; webview-show guards.
