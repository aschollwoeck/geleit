# S3.6 — HTML fidelity + perf · Tasks

## Build
- [x] safehtml rewrite: keep styles/classes/<style>/font/links/cid+data; block remote img; layered model
- [x] app: ensure_webview (build once at startup, pre-paint bg → no black flash); show_html slimmed
- [x] root [profile.release] strip + thin-LTO
- [x] manual update (formatting now shown)

## Verify
- [x] AC1 build/test/clippy/fmt/deny green
- [x] AC2 sanitizer keeps formatting/links/inline images, strips scripts + remote img (tested)
- [x] AC3 document() CSP intact (script blocked; img-src opt-in only)
- [~] AC4 first mail open shows formatted content, no black flash (MAINTAINER eyeballs)
- [x] AC5 mutants 0 missed; release binary ~30M

## Ship
- [x] Code review (security): layered model sound (no default remote-load / no script path). Fixed
      the one Medium — `data:`/unsafe schemes stripped from `href` + a navigation handler opens real
      links in the system browser (no in-pane nav away from the CSP'd doc). Cue-misses-CSS-remote
      noted as a follow-up (CSP blocks the load regardless).
- [x] tasks all-done; PR merged
