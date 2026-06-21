# S3.1 — Sandboxed HTML reading · Spec (the WHAT)

Slice of **M3**. Type: engine + UI (the webview integration). Delivers **READ-4** (read real HTML
mail, rendered) with **PRIV-1** (remote content blocked) + **PRIV-4** (no script execution) — via a
**sandboxed `wry` webview embedded in the reading pane**, showing engine-**sanitized** HTML.
Feasibility proven by the `spikes/s3.1-html-embed` spike (X11: webview embeds + GTK-pumped + ammonia
strips scripts/remote).

Status: **draft.**

## Purpose
When a message has HTML, show it **formatted** in a sandboxed webview — with trackers/remote images
and scripts already stripped — instead of the plain-text fallback. Plain-text messages keep the
existing text reading pane.

## In scope
- `geleit-engine::safehtml::sanitize_html` (ammonia: strip `<script>`, event handlers, and **all
  remote URL schemes** — only `mailto`/`cid` survive) — pure, unit-tested (the security core).
- `geleit-app`: an embedded `wry` webview over the reading-pane body for HTML messages; sanitized
  HTML loaded via `load_html`; shown/positioned for HTML, hidden for text/none. GTK loop pumped
  under Slint's event loop. (X11; deps: slint `unstable-winit-030`, `wry`, `gtk`.)

## Out of scope
- **CSP belt-and-suspenders** + sandbox-escape tests (S3.2 — the webview runs page JS, so
  sanitization is currently the sole guarantee; hardening next). Per-message "load remote content"
  / trusted sender (S3.3). **Wayland** (`build_as_child` is X11-only; the GTK-container path is a
  follow-up). Pixel-perfect overlay of the header/attachments (the webview covers the body region;
  refinement follows).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green (ammonia/wry/gtk licenses pass).
2. `sanitize_html`: `<script>` removed; remote `http(s)` img/link refs removed; `on*` handlers
   removed; safe tags + text + `mailto:` kept — unit-tested (the PRIV-1/PRIV-4 guarantee).
3. The app builds + launches; opening an HTML message renders it in the webview, a plain-text
   message shows the text pane (webview hidden). **Visual fidelity confirmed by the maintainer
   running it** (the one part not machine-verifiable).
4. P1 unchanged (no network on the read path — sanitized HTML is rendered from the local store; the
   webview is configured remote-blocked via sanitization).
5. `cargo mutants` — `sanitize_html` covered; `main.rs` excluded; 0 missed.

## Deliverables
- `engine::safehtml::sanitize_html` + tests; webview-in-reading-pane integration; `docs/technical/`
  spike finding; manual touch. *(ADR-0001's embedded webview is now actually realised.)*
