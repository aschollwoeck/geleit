# S3.2 — HTML hardening · Plan (the HOW)

## geleit-engine::safehtml
- `pub fn document(sanitized_body: &str) -> String`: `<!doctype html><html><head><meta charset> +
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data: cid:;
  style-src 'unsafe-inline'; font-src data:"> + <style>(calm base)</style></head><body>{body}</body>`.
  CSP `<meta>` is in the trusted wrapper (sanitized content strips `<meta>`), so it's authoritative.
- More escape tests on `sanitize_html`: `javascript:` href, `<svg onload=…>`, `formaction`,
  malformed/nested, `style="…expression()…"` → all neutralised.

## geleit-app
- `show_html`: render `safehtml::document(sanitized)` instead of the bare sanitized body.

## Verify
gates; `document` test (CSP present, body present); escape tests; mutants; launch.
