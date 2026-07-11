# ADR-0012: Tauri shell (system webview) with a Leptos UI, mail in a sandboxed iframe

## Status
**Accepted** (M9). **Supersedes [ADR-0001](0001-native-slint-with-sandboxed-webview-for-html.md)**
(Slint shell + embedded webview component) and **[ADR-0011](0011-html-via-blitz-cpu-rendering.md)**
(Blitz CPU HTML rendering). Retires [ADR-0007](0007-slint-licensing.md) (Slint licensing), which is
moot once Slint is removed. Required amending **constitution P4**, which previously read "Native,
not a webview shell".

## Context

The reading pane is the product. GeleitMail exists to read mail, and for years of accumulated
convention that means rendering arbitrary, hostile, table-soup HTML *the way every other mail client
renders it*. We tried twice to do that without a browser engine, and both attempts failed on their
own merits — not for lack of effort.

**Attempt 1 — embedded webview component (ADR-0001).** A `wry`/WebKitGTK webview embedded as a child
of the Slint window, per P4's "contained exception". It crashed: `GLXBadWindow`, raised from winit's
X11 error handler when the webview's asynchronous GL errors surfaced on the shared X11 connection.
Size-gating the child window (PR #93) did not fix it. Switching Slint to its software renderer so it
held no GL context of its own did not fix it either — the X11 connection is shared regardless. The
crash was in the *embedding*, and we could not make the embedding stable.

**Attempt 2 — pure-Rust CPU rendering (ADR-0011).** Blitz (Stylo + Taffy + Parley, pre-alpha
`0.3.0-alpha.5`). It rendered *something* — enough to look promising on synthetic demos — but could
not render real mail. Against a real message we found and hand-fixed five separate defects (MIME
header decoding, images requiring a `NetProvider`, dropped digit glyphs on uninstalled fonts,
tall-image display, phantom `border-collapse` borders) and the result was still visibly wrong. The
work degenerated into chasing renderer bugs one at a time in a pre-alpha crate.

The premise underneath P4 — that a *contained* native/webview split was achievable — turned out to
be false on this stack. The real choice was never "native shell + contained webview" versus "webview
shell". It was **correct rendering** versus **broken rendering**.

## Decision

- The application shell is **Tauri v2**, which uses the **operating system's webview** (WebKitGTK on
  Linux, WebView2 on Windows, WKWebView on macOS). No browser is bundled.
- The UI is **Leptos** (CSR), compiled Rust → WASM. **The app stays Rust end to end; there is no
  npm, no `package.json`, no JS toolchain.** `cargo` and `deny.toml` keep covering the whole tree.
- **A message is never rendered in the app's own document.** It is confined to an `<iframe>` with:
  - `sandbox="allow-popups allow-popups-to-escape-sandbox"` — crucially **no `allow-scripts`** and
    **no `allow-same-origin`**, so mail cannot execute code, reach the DOM of the shell, touch the
    Tauri IPC bridge, or read the filesystem;
  - a strict **CSP** (`default-src 'none'; img-src data: cid:; style-src 'unsafe-inline';
    font-src data:; form-action 'none'; base-uri 'none'`), so nothing remote is fetched;
  - the existing **ammonia sanitizer** still applied first, as defense in depth.
- **Remote images (PRIV-2) become a CSP relaxation**, not an HTTP client. "Load images" re-renders
  that one message with `img-src` widened to `https:`, and WebKit fetches. `remoteimg.rs` and the
  `ureq` dependency are deleted, and `ureq` is added to the `deny.toml` ban list — after this change
  the app has **no HTTP client at all**. The webview's network context is configured ephemeral (no
  cookie jar, no persistent cache) so image loads cannot be correlated across sessions.
- **Links never navigate the app.** A click surfaces as a new-window request, which the shell denies
  and hands to the system browser.

## Consequences

**Good**
- Mail renders **correctly** — verified against a real message that Blitz could not render: correct
  fonts, digits, table layout, image sizing, rounded buttons, and zero phantom borders, with **no
  workarounds at all**.
- Text selection, scrolling, link handling, and image sizing are free and native, instead of ~330
  lines of hand-rolled Blitz plumbing.
- **Deleted:** `htmlrender.rs` (237 lines), `remoteimg.rs` (91), the `blitz-*` stack, `ureq`, Slint,
  and the two Blitz workarounds in `safehtml.rs`. One of those workarounds
  (`table{border-collapse:separate!important}`) is *actively wrong* for a real engine and would have
  corrupted every email that legitimately collapses its borders.
- The **security posture improves.** Verified by feeding hostile payloads *with the sanitizer
  bypassed*: inline `<script>`, `img onerror`, `svg onload`, `body onload` were all inert; a nested
  tracker iframe, a 1×1 pixel, remote CSS, and a CSS `url()` tracker were never fetched; a
  `<form action="https://evil…">` could not submit. Three independent layers (sanitizer, sandbox,
  CSP) each hold the line alone, and enforcement now comes from a hardened engine rather than from
  our own renderer's incidental limitations.
- `geleit-core`, `geleit-platform`, `geleit-store`, `geleit-engine` — **6,069 lines**, all the
  IMAP/SMTP, encrypted store, MIME, search and threading — are **untouched**. Only `geleit-app`
  is rewritten. The crate boundary paid for itself.

**Bad — and accepted deliberately**
- **Cold start regresses ~700 ms.** Measured on the target machine (i7-2600, X11): Slint painted at
  ~275 ms; the webview paints at ~1000 ms, of which **~630 ms is WebKitGTK spawning its web process
  before any of our code runs**. This is inherent to any webview and is unavoidable. It is a
  once-per-launch cost, not per-interaction. Mitigation: paint a skeleton immediately so the window
  is never blank. Constitution P4 now caps cold start at **1200 ms**, enforced in CI.
- **Idle RSS rises** from ~60 MB to an expected ~150–250 MB. Capped at **280 MB**, enforced in CI.
- **The "native, not a webview shell" brand claim is gone.** `vision.md`, `GELEITMAIL.md`, and the
  user manual are corrected. What survives is defensible and true: Rust end to end, no bundled
  browser, no telemetry, no HTTP client, local-first.
- **WebKitGTK becomes a Linux runtime dependency** (`libwebkit2gtk-4.1`, standard on Ubuntu/Fedora;
  the package declares it).

**Measured, not assumed** (i7-2600 / Sandy Bridge, release builds — a deliberately pessimistic
floor; modern hardware should be ~2–3× faster):

| Stage | Leptos/WASM | Vanilla JS |
|---|---|---|
| Webview created | 190 ms | 199 ms |
| WebKit booted, HTML parsed | 817 ms | 833 ms |
| Runtime entered (wasm fetched+compiled+instantiated) | 831 ms | 836 ms |
| 300-row reactive list mounted | 869 ms | 842 ms |
| **First paint** | **1009 ms** | **984 ms** |

**WASM costs ~25 ms** — the 212 KB blob instantiated in 10 ms. The frontend stack was therefore
*not* a performance decision, and Leptos was chosen on ethos: Rust end to end, zero npm, and a
dependency tree `deny.toml` still fully covers.

## Alternatives considered

- **Stay on Slint + Blitz.** Rejected: Blitz is pre-alpha and cannot render real mail. This is the
  status quo we are leaving, for cause.
- **Tauri + Svelte/TypeScript.** Rejected: introduces npm and a second supply chain to audit into a
  privacy-first app that has been pure `cargo`. It is ~6× faster at bulk DOM building (6 ms vs 38 ms
  for 300 rows), but that is irrelevant once the list is virtualized.
- **Tauri + vanilla HTML/CSS/JS.** Rejected: tightest supply chain, but ~4,000 lines of hand-rolled
  DOM and state management to own forever.
- **Dioxus Desktop** (same `wry` webview, Rust UI, no Tauri). Rejected: loses Tauri's bundler,
  updater, and capability/permission model, which a shipping app wants.
- **Keep the native shell, put the webview in a separate top-level window.** Rejected: dodges the
  embedding crash but splits the message out of the app — unacceptable UX.

## Verification
Spikes under `spikes/tauri-reading-pane/` (rendering + hostile-payload security) and
`spikes/wasm-coldstart/` (cold-start measurement); findings in
`docs/technical/tauri-webkit-spike.md`. Both were run against the maintainer's real message, which
is **not committed** (personal mail; `*.eml` is gitignored).
