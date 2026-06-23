# Backlog — Render HTML mail with Blitz (CPU), replacing the webkit webview

The embedded webkit (GL) webview kept crashing on X11 (GLXBadWindow → winit panic, #91/#92/#93),
even on large windows, and couldn't be reproduced locally to verify a fix. Replace it with a
pure-Rust, GL-free renderer. See ADR-0011.

## Decision
Render sanitized HTML to a CPU bitmap with Blitz + anyrender_vello_cpu; show it as a Slint Image in
the reading pane; keep the DOM so link clicks are hit-tested and opened in the system browser.

## Acceptance criteria
1. HTML mail renders correctly in-app (headings, bold/italic, colors, tables, links). [verified by
   screenshot + the blitz_spike PNG]
2. No GL → the previously-crashing 500px window no longer crashes. [verified]
3. Links are clickable (hit-test → xdg-open). [wired; drag-click not injection-testable here]
4. Remote content blocked by default (no net provider); cue shown when present (PRIV-3). [done]
5. wry + gtk removed; build/test/clippy -D warnings (+dangerous-tls)/fmt/cargo deny green. [done]

## Follow-ups (not blocking)
- hi-dpi crisp render (render at physical px); re-render on resize; PRIV-2 per-message remote opt-in
  on the new renderer; text selection; catch_unwind → text fallback on a Blitz panic.

## Deliverables
- crates/geleit-app/src/htmlrender.rs (render + link_at); reading-pane Image + html-click; ADR-0011;
  examples/blitz_spike.rs.
