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
- [x] Code review agent on the diff — 6 findings, all acted on (below)
- [x] Manual already accurate (updated in the M9 decision PR); technical doc updated

## Code review — findings acted on
The core architecture was found sound (id parsed strictly as `i64` + bound as a parameter → no
injection; body genuinely never crosses IPC; sandbox/incognito/CSP-test all correct). Six refinements:

| # | Finding | Fix |
|---|---|---|
| 1 | **CSS `@import "http://…"` (string form) evaded the "remote blocked" cue** — the same bug-class this slice claimed to close. `css_references_remote` only scanned `url(`. | Now also scans the `@import` string form. Tested, including through a full message. |
| 2 | **`allow_navigation` allowed *any* `*.localhost` in-window** — a mail link to `http://ipc.localhost/` could navigate the app's own IPC origin (and the popup escaped the sandbox). | Tightened to an **exact host allowlist** (`tauri`/`ipc`/`mail`.localhost); every other loopback host goes to the browser. Tested. |
| 3 | **Windows `cmd /C start <url>` with an attacker URL** — BatBadBut/CVE-2024-24576 metacharacter class. | Switched to `explorer.exe <url>` (single argv, no shell re-parse) + refuse cmd metacharacters. Windows isn't a shipping target yet; noted to revisit with `ShellExecuteW`. |
| 4 | **`data:`/`blob:`/`about:` were navigable** — a top-level `data:text/html,<script>` runs under an opaque origin (no live trigger, but a removed backstop). | Dropped from the allowlist; refused outright. Tested. |
| 5 | **Opt-in widened `img-src` to `http:` too** — the ADR/spec say `https:`; cleartext beacons are on-path visible. | Tightened to **`https:` only** in both `webview_document` and `mailproto::csp` (a test keeps them identical). Re-verified images still load. |
| 6 | **Three ammonia passes per HTML open** (`has_remote_content` × 2 + render × 1). | Added a cheap-reject guard so plain mail (no scheme, no `url(`) skips the two detection passes entirely. |

## Found & fixed while building this slice
- **A CSS-based tracker (`style="background:url(http://…)"`) was never surfaced to the user.** The
  Slint app decided "remote content blocked" purely by comparing the two sanitizations, but the
  sanitizer leaves CSS alone — so a tracker hiding in CSS was *blocked* by the CSP yet the user was
  never *told*, and "Load images" never appeared for it. `has_remote_content` now also scans CSS.

## Not in scope
Virtualization/threading/flags (S9.3) · refresh (S9.4) · compose (S9.5) · search/settings (S9.6) ·
deleting Slint+Blitz and `document`'s workarounds (S9.7) · CI perf budgets (S9.8).
