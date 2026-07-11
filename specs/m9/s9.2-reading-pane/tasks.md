# S9.2 — Tasks

Status: **complete** — all gates green; verified in-app against the maintainer's real `.eml` **and** a
hostile payload.

## Engine (`geleit-engine::safehtml`)
- [x] `webview_document` — strict CSP, **no Blitz workarounds** (left `document` untouched for Slint)
- [x] `has_remote_content` — sanitize-diff **plus** a CSS `url()` scan (a CSS tracker is invisible to
      the diff; the CSP blocks it either way, but the user should still be told)
- [x] Tests: remote detected only when something was stripped · CSP blocks images by default · opt-in
      widens only `img-src` · **no `script-src` ever** · no border-collapse/font hacks · CSS url()
      local-vs-remote (mutation-covered)

## Shell (`geleit-shell`)
- [x] `mailproto` — serves `mail://localhost/<id>[?images=1]`; body **never** returned to the frontend
- [x] CSP sent as a **header too**, identical to the document's own (a test enforces they match)
- [x] `main::allow_navigation` — app never navigates; `http(s)`/`mailto` → system browser (`xdg-open`
      subprocess, no HTTP client, no capability); `localhost`-lookalike hosts refused; other schemes
      refused. Unit-tested.
- [x] Window built in `setup()` so the navigation guard can be attached
- [x] `incognito(true)` (no cookie jar / cache — image loads can't be correlated)
- [x] `open_message` returns `is_html` + `has_remote`; the HTML body stays server-side
- [x] CSP: `frame-src mail: http://mail.localhost` (the only place the app embeds a frame)

## Frontend (`geleit-ui`)
- [x] HTML message → sandboxed `<iframe src="mail://localhost/<id>">` (no allow-scripts, no
      allow-same-origin); plain-text unchanged
- [x] "Remote content blocked" cue + **Load images** button (PRIV-3/PRIV-2)
- [x] Opt-in is **strictly per message** — reset on every `open`, so one click never carries over
- [x] iframe fills the pane and scrolls internally (no double scrollbar)

## Verified in-app (screenshots)
- [x] The maintainer's real TF Bank `.eml`: decoded subject, correct fonts/**digits**/layout, rounded
      button, **no black borders** — the render Blitz never achieved, with **zero workarounds**
- [x] Images **blocked** by default with the cue shown; **loaded** on opt-in (logo + hero appear)
- [x] **Hostile `.eml` through the full shipping pipeline** (sanitizer + mail:// + sandbox + CSP):
      inline `<script>`, `img onerror`, `svg onload`, tracker iframe, 1×1 pixel, form, CSS `url()`
      tracker, `javascript:` link — **all inert, no PWNED banner**

## Gates
- [x] fmt · clippy `-D warnings` · tests · `cargo deny` (advisories/bans/licenses/sources ok)
- [x] wasm build · mutants on the new pure logic (`css_references_remote` fully covered)
- [ ] Code review agent on the diff → then merge
- [x] Manual already accurate (updated in the M9 decision PR); technical doc updated

## Found & fixed while building this slice
- **A CSS-based tracker (`style="background:url(http://…)"`) was never surfaced to the user.** The
  Slint app decided "remote content blocked" purely by comparing the two sanitizations, but the
  sanitizer leaves CSS alone — so a tracker hiding in CSS was *blocked* by the CSP yet the user was
  never *told*, and "Load images" never appeared for it. `has_remote_content` now also scans CSS.

## Not in scope
Virtualization/threading/flags (S9.3) · refresh (S9.4) · compose (S9.5) · search/settings (S9.6) ·
deleting Slint+Blitz and `document`'s workarounds (S9.7) · CI perf budgets (S9.8).
