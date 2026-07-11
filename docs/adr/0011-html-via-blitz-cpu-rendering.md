# ADR-0011: Render HTML mail with Blitz on the CPU (replacing the embedded webkit webview)

Status: **Superseded by [ADR-0012](0012-tauri-shell-with-leptos-ui.md)** (M9). Blitz is pre-alpha and
could not render real mail correctly: against a real message we hand-fixed five separate defects and
the result was still visibly wrong. HTML is now rendered by the OS webview. Previously: Accepted —
superseded the embedded-webview decision (ADR-0001 / S3.1).

## Context
HTML mail was rendered by an embedded **webkit2gtk webview** (wry, `build_as_child` into the Slint
window, X11-only). It used GL, and coexisting GL-on-X11 with Slint proved fundamentally fragile —
it caused a long series of crashes: eager-build GLXBadWindow (#52), the move to a software Slint
renderer (#53), a degenerate-size `BadValue` abort from the splitter (#91), a separator overlap
(#92), and finally a winit `set_theme → flush_requests().expect()` **panic** surfacing a stray async
`GLXBadWindow` from the webview's GL (#93) that recurred even on large windows and that we could not
reproduce locally to verify a fix. The webview was also a floating native child window (no native
compositing with Slint overlays, awkward geometry).

## Decision
Drop the embedded webview entirely. Render sanitized mail HTML to a **CPU bitmap** with **Blitz**
(`blitz-html` + `blitz-dom` + `blitz-paint`) via **`anyrender_vello_cpu`** (vello_cpu — no GPU/GL),
and display it as an ordinary Slint `Image` in the reading pane. Keep the Blitz DOM so link clicks
are **hit-tested** (`document.hit` → nearest `<a href>` → open in the system browser).

## Consequences
- **The entire GL-on-X11 crash class is gone** — no GL, no native child window. Verified: the 500px
  window that always crashed is now stable, and HTML renders correctly in-app.
- Pure-Rust, in-process, no external runtime (vs. a headless browser); privacy preserved (the
  renderer is given **no network provider**, so trackers/remote images cannot load).
- `cargo deny` passes with the Servo/Stylo dependency tree (licenses/advisories/bans/sources).
- Removed deps: `wry`, `gtk`, and Slint's `unstable-winit-030` feature.
- Trade-offs (follow-ups): Blitz is **pre-alpha** (complex emails may render imperfectly); renders at
  logical px scale 1.0 (slightly soft on hi-dpi); no re-render on resize yet; PRIV-2 per-message
  "load remote images" opt-in is deferred (remote is always blocked — the safe default); a bitmap has
  no text selection; add a `catch_unwind` text fallback around the render.

## Alternatives considered
- **Headless Chromium** (chromiumoxide) → PNG: best fidelity, but requires Chromium installed + a
  subprocess; against the minimal/local-first grain.
- **Keep the webview, swallow the X error**: couldn't reproduce locally to verify; winit consumes the
  error via its own xcb queue, so a global Xlib handler wouldn't reliably prevent the panic.
