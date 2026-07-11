# S9.2 — Plan

## The mechanism, end to end

```
open a message
  frontend: open_message(id)  ->  { is_html, has_remote, plain, ... }   (NO html body)
  if is_html:
     <iframe sandbox="allow-popups allow-popups-to-escape-sandbox"
             src="mail://localhost/<id>[?images=1]">
  shell: mailproto::handle  ->  message_html(store, id, allow_remote)
             sanitize_html[_allowing_remote]  ->  webview_document  (own CSP, no Blitz hacks)
         served with an identical CSP response header
```

The body is fetched **inside the protocol handler**, from the store, and handed straight to WebKit.
It is never returned over IPC, so hostile markup never touches the app's own document.

## Why each piece is where it is

- **Custom `mail://` scheme, not `srcdoc`:** a `srcdoc` iframe inherits the embedder's CSP, which
  would strip every message's inline styles (the app CSP has no `style-src 'unsafe-inline'`). Its own
  origin lets the message carry its own CSP. (This trap was found in the S9.1 review and is why the
  scheme exists.)
- **Window built in `setup()`:** `on_navigation` can only be set at build time, and without it a link
  in a message could navigate the *app*. So the window moves out of `tauri.conf.json` into code.
- **`webview_document` separate from `document`:** the Slint app still needs `document` (with its two
  Blitz workarounds) until S9.7. Adding a clean function beside it avoids touching a shipping path.
- **`has_remote_content` scans CSS as well as diffing sanitizations:** the sanitizer leaves CSS
  alone, so a `url(http://…)` tracker is invisible to the diff — blocked by the CSP, but the user was
  never told. Both signals together make the cue honest.

## Tests

- **Engine:** the CSP/no-workaround/opt-in properties of `webview_document`; `has_remote_content`
  including the CSS path; `css_references_remote` local-vs-remote (mutation-covered).
- **Shell:** `allow_navigation` (own origins load; remote links/`mailto` refused → browser; lookalike
  `localhost` hosts refused; other schemes refused); `mailproto` header==meta CSP; placeholder inert.
- **In-app:** the maintainer's real `.eml` (render + images blocked/loaded) and a **hostile** `.eml`
  (every vector inert) — screenshots, since the render is the whole point and only eyes confirm it.

## Risks

| Risk | Handling |
|---|---|
| `srcdoc` CSP-inheritance trap | Avoided by design (own origin); recorded in the roadmap + tech doc |
| Header/meta CSP drift | A test asserts they are identical |
| Opt-in leaking across messages | Reset on every `open`; per-message by construction |
| Can't inject clicks to test in-app | Debug-only `GELEIT_OPEN` / `GELEIT_IMAGES` seams (compiled out of release) |
