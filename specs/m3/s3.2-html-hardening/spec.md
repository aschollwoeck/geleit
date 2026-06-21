# S3.2 — HTML hardening (CSP + escape tests) · Spec (the WHAT)

Slice of **M3**. Type: engine + UI glue. Hardens the S3.1 HTML viewer with **defense-in-depth**:
a strict Content-Security-Policy on the rendered document so that even a sanitizer miss can't load
remote content or run scripts (PRIV-1/PRIV-4), plus a wider sandbox-escape test suite.

Status: **draft.**

## Purpose
S3.1's safety rests on `ammonia` sanitization + JS-disabled. S3.2 adds a second wall: render the
sanitized body inside a trusted document carrying a `default-src 'none'` CSP, and pin the
sanitizer's guarantees with adversarial tests.

## In scope
- `geleit-engine::safehtml::document(sanitized_body) -> String`: wrap the sanitized body in a
  minimal HTML document with a strict CSP `<meta>` (`default-src 'none'; img-src data: cid:;
  style-src 'unsafe-inline'; font-src data:`) + calm base styling. The CSP `<meta>` lives in the
  **trusted wrapper** (not the sanitized content, which strips `<meta>`).
- App renders `document(sanitize_html(html))` in the webview.
- Sandbox-escape tests for `sanitize_html`: `javascript:` href, `<svg onload>`, malformed/nested
  markup, `formaction`, `style` `expression()` — all neutralised.

## Out of scope
- Per-message "load remote content" / trusted sender (S3.3). CSS-aware fidelity (follow-up). Wayland.

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. `document()` wraps with a `default-src 'none'` CSP (no `script-src`/remote allowed) and includes
   the body — tested.
3. Sandbox-escape tests: `javascript:`/`<svg onload>`/`formaction`/nested vectors all stripped — tested.
4. App renders via `document(...)`; launches clean. (Visual still maintainer-eyeballed.)
5. `cargo mutants` — `document` + `sanitize_html` covered; `main.rs` excluded; 0 missed.

## Deliverables
- `safehtml::document` + CSP; expanded escape tests; app wiring; manual touch.
