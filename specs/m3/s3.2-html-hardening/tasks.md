# S3.2 — HTML hardening · Tasks

## Build
- [x] engine `safehtml::document` (CSP wrapper) + test
- [x] engine: sandbox-escape tests (javascript:/svg onload/formaction/nested)
- [x] app: render `document(sanitize_html(..))` in the webview
- [x] manual touch

## Verify
- [x] AC1 build/test/clippy/fmt/deny green
- [x] AC2 document() has default-src 'none' CSP + body (tested)
- [x] AC3 escape vectors stripped (tested)
- [x] AC4 app renders via document(); launches clean
- [x] AC5 mutants 0 missed (document + sanitize_html)

## Ship
- [x] Code review (guidelines §11)
- [x] tasks all-done
- [x] PR merged
