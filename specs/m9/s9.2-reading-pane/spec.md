# S9.2 — The reading pane: formatted mail, safely

**Milestone:** M9. **Decision:** [ADR-0012](../../../docs/adr/0012-tauri-shell-with-leptos-ui.md).
**Constitution:** P2 (privacy is the product), P4 (as amended), P6 (integrity; HTML rendering is a
security-critical path), P3 (calm and fast).

**This is the slice the whole milestone exists for.** Everything else in M9 is scaffolding around it.

## What it delivers

Formatted (HTML) mail renders **correctly** in the reading pane — the thing neither the embedded
webview (ADR-0001) nor Blitz (ADR-0011) ever managed — and hostile mail is **provably inert**.

| | Story | Acceptance |
|---|---|---|
| **S9.2-1** | I read a formatted email and it looks like the sender meant it to. | The real `.eml` that Blitz could not render (fonts, digits, table layout, image sizing, rounded buttons, **no black borders**) renders correctly. |
| **S9.2-2** | I can select and copy text, and scroll. | Native — text selects, `Ctrl+C` copies, the message scrolls. |
| **S9.2-3** | Clicking a link opens my browser. | `http(s)`/`mailto` links open in the system browser; the app itself **never navigates**. |
| **S9.2-4** | Nothing loads from the internet unless I ask. | Remote images, tracking pixels, remote CSS/fonts are **blocked**; a "Remote content blocked" cue appears with a **Load images** button (PRIV-2/PRIV-3). |
| **S9.2-5** | A malicious email cannot do anything. | Scripts cannot run, cannot reach the app or its IPC bridge, cannot read the filesystem, cannot submit forms — **even if the sanitizer is bypassed** (PRIV-4). |
| **S9.2-6** | Plain-text mail still reads plainly. | Unchanged from S9.1. |

## How — and the one decision that matters

**The message is served from its own origin (`mail://`), not from `srcdoc`.**

This is not a stylistic choice. A `srcdoc` iframe **inherits the embedding document's CSP**, and the
app's CSP is deliberately strict (`style-src 'self'`, no `'unsafe-inline'`). Mail delivered via
`srcdoc` would therefore have **every message's inline styles silently stripped** — all mail would
render unstyled, and the cause would be invisible. Serving from a custom scheme gives the message its
own origin, so it carries exactly the CSP we hand it and inherits nothing.

Three independent layers, each of which holds the line alone (proven in the spike, with the sanitizer
switched *off* — `docs/technical/tauri-webkit-spike.md`):

1. **Sanitizer** — `ammonia`, as today.
2. **Sandbox** — `sandbox="allow-popups allow-popups-to-escape-sandbox"`: **no `allow-scripts`**, **no
   `allow-same-origin`**. Mail cannot execute code, reach the shell's DOM, touch the IPC bridge, or
   read files.
3. **CSP** — `default-src 'none'; img-src data: cid:; style-src 'unsafe-inline'; font-src data:;
   form-action 'none'; base-uri 'none'`. Nothing remote is fetched. `img-src` is the **only**
   directive ever relaxed, and only on explicit opt-in.

**"Load images" is a CSP relaxation, not an HTTP client.** Asking for images re-serves that one
message with `img-src` widened to `https:` and lets WebKit fetch. `remoteimg.rs` and `ureq` become
dead (deleted in S9.7) — after M9 the app has **no HTTP client at all**.

## Blitz workarounds must not leak in

`safehtml::document()` carries two hacks that exist *only* because Blitz was broken:

- `table{border-collapse:separate!important}` — **actively wrong** for a real engine; it would corrupt
  every email that legitimately collapses its table borders.
- `add_font_fallbacks()` — Blitz dropped digit glyphs for uninstalled fonts.

The Slint app still needs them until S9.7. So S9.2 adds a **separate** `safehtml::webview_document()`
without them, and leaves `document()` alone. S9.7 deletes `document()` and the hacks together.

## Out of scope

Virtualization/threading/flags (S9.3), refresh (S9.4), compose (S9.5), search/settings (S9.6),
deleting Slint+Blitz (S9.7), CI perf budgets (S9.8). Attachments remain view-only.
