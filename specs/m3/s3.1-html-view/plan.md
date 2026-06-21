# S3.1 — Sandboxed HTML reading · Plan (the HOW)

Implements `spec.md`. Builds on the proven `spikes/s3.1-html-embed`.

## geleit-engine::safehtml (pure, mutation-tested)
- Dep: `ammonia`.
- `pub fn sanitize_html(raw: &str) -> String`: `ammonia::Builder::default().url_schemes({mailto,cid})
  .clean(raw)`. Default ammonia already strips `<script>` + `on*` handlers; restricting URL schemes
  drops every `http(s)` ref (remote images/links) → PRIV-1. Returns sanitized HTML.
- Tests: script removed; `http(s)` img/link removed; `onclick` removed; `<b>/<p>/<h1>` + text +
  `mailto:` kept.

## geleit-app (webview integration)
- Deps: `slint` features `unstable-winit-030` + `backend-winit-x11`; `wry`; `gtk` (0.18).
- `main`: `gtk::init()` first. Hold `webview: Rc<RefCell<Option<wry::WebView>>>`.
- A repeated `slint::Timer` (~16ms): pump GTK (`while gtk::events_pending() { gtk::main_iteration_do
  (false) }`) and, when the webview is visible, keep its bounds on the reading-pane **body** region
  (computed from `ui.window().size()`/`scale_factor()`; x≈ rail+list = 620 logical, below the
  subject/sender header).
- `show_html(ui, webview, html)`: lazily build the child webview (`with_winit_window` →
  `WebViewBuilder…build_as_child`) the first time; then `load_html(sanitize_html(html))`,
  `set_bounds(rect)`, `set_visible(true)`.
- `hide_html(webview)`: `set_visible(false)`.
- `on_message_selected`: if `body_for(id).html` is `Some` → `show_html` with the sanitized HTML;
  else `hide_html` (text pane shows). `folder-selected` / reload / setup → `hide_html`.

## Verify
gates; `sanitize_html` unit tests + mutants; **launch + maintainer eyeballs** an HTML vs a text
message; deny (new licenses). `.cargo/mutants.toml`: `main.rs` already excluded; `safehtml` covered.
Keep the spike under `spikes/` as evidence.
