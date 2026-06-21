# S4.6 — Basic formatting via Markdown (SEND-6) · Spec

Slice of **M4** (final). Compose with **bold, lists, links** etc. by writing **Markdown** — readable
as plain text, and sent as a `multipart/alternative` (text/plain = the Markdown, text/html = rendered)
when the "Format with Markdown" toggle is on. Fits the privacy-first, plain-text-friendly ethos
without a rich-text editor.

## In scope
- Engine: `Draft.html_body: Option<String>`; `build()` adds it as the text/html part (multipart/
  alternative). `render_markdown(md)` via `pulldown-cmark` (strikethrough + tables on). Pure, tested.
- App: a "Format with Markdown" toggle in compose; when on, `run_send` renders the body to HTML.

## Out of scope
- WYSIWYG/rich-text editor; HTML for replies' quoted parts beyond what Markdown yields; per-account
  default. Toggle resets off on a new compose.

## Acceptance criteria
1. build/test/clippy -D warnings/fmt/`cargo deny check` green.
2. `render_markdown` produces bold/links/lists/strikethrough/tables; a Draft with `html_body` builds a
   `multipart/alternative` (text + html parts) — tested.
3. App: toggle on → the sent message carries an HTML alternative; off → plain text only (maintainer
   eyeballs).
4. `cargo mutants` — `message` 0-missed.

## Deliverables
- `render_markdown` + `Draft.html_body` + build + tests; compose toggle + `run_send` wiring;
  `pulldown-cmark` dep (no-default + `html`).
