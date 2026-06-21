# S3.1 — Sandboxed HTML reading · Tasks

Derived from `spec.md` + `plan.md` (P8). Engine + UI (webview) slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build
- [x] engine `safehtml::sanitize_html` (ammonia) + tests (script/remote/handler stripped; safe kept)
- [x] app deps: slint `unstable-winit-030`+`backend-winit-x11`, `wry`, `gtk`
- [x] app: embedded webview lifecycle (build_as_child, load_html, show/hide, bounds), GTK pump timer
- [x] wire `on_message_selected` (HTML → webview, text → hide); hide on folder/reload/setup
- [x] keep the spike under `spikes/`; `docs/technical/` finding; manual touch

## Verify (acceptance criteria)
- [x] AC1 build/test/clippy/fmt/`cargo deny check` green
- [x] AC2 sanitize_html strips script/remote/handlers, keeps safe (tested)
- [~] AC3 app launches clean; webview wiring verified by the proven spike; **visual fidelity of
      rendered mail still to be eyeballed by the maintainer on a running window** (the one part not
      machine-verifiable)
- [x] AC4 P1 unchanged (no network on read; remote blocked via sanitization)
- [x] AC5 mutants — sanitize_html covered; main.rs excluded; 0 missed

## Ship
- [x] Code review (guidelines §11) — verdict sound; fixed both Medium findings: scheme-relative
      `//host` URLs now stripped (`url_relative(Deny)`) and JS disabled in the webview
      (`with_javascript_disabled`) + gap tests + gtk-init guard. CSS-aware sanitization (fidelity)
      and a `//host`-style follow-up noted for S3.2.
- [x] Update this tasks file to all-done
- [x] PR merged (one-slice-one-PR, §12)