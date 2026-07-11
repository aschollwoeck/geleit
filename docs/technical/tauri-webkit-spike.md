# Spike: OS webview (WebKitGTK) for the reading pane — findings

Time-boxed feasibility spike for **M9 / ADR-0012**: can we abandon native HTML rendering and put the
whole shell in the OS webview? Two throwaway crates, both excluded from the workspace:

- `spikes/tauri-reading-pane/` — rendering fidelity + security
- `spikes/wasm-coldstart/` — cold-start cost (Leptos/WASM vs vanilla JS)

Both use `wry` + `tao` directly. That is the **same WebKitGTK engine Tauri wraps**, so the results
transfer, and it let the spike skip Tauri's scaffolding entirely.

> The spike was run against the maintainer's **real** message. That file is personal mail: it is
> gitignored (`*.eml`) and **must never be committed**, and neither may screenshots of it. Every
> earlier synthetic demo hid the bugs that the real message exposed — test with real mail.

## Environment

Deliberately the worst realistic case, so the numbers are a floor rather than a best case:

| | |
|---|---|
| CPU | Intel i7-2600 (Sandy Bridge, 2011), 8 threads |
| Session | X11 |
| WebKitGTK | webkit2gtk-4.1, 2.52.3 |

## Gate 1 — does it crash? **No.**

The `GLXBadWindow` crashes that killed ADR-0001 came from **embedding** a webview as a child of the
Slint window: the webview's asynchronous GL errors surfaced on the shared X11 connection and were
raised by winit's error handler. Neither size-gating the child nor switching Slint to its software
renderer fixed it.

As a **top-level** window there is nothing to collide with — the window *is* the webview. Repeated
launches, stable, no crash.

## Gate 2 — does real mail render correctly? **Yes, with zero workarounds.**

The message that Blitz never managed rendered correctly on the first try: correct fonts and **digits**
("Bis zu 15.000 € · Laufzeit bis 10 Jahre · 100 % online"), correct table layout, correctly sized
images, rounded buttons, and — the maintainer's core complaint throughout M3 — **no black borders
anywhere**.

Critically, the spike deliberately **omits both Blitz workarounds** from `safehtml::document()`:

- `add_font_fallbacks()` — added because Blitz/parley dropped digit glyphs for named-but-uninstalled
  fonts. Unnecessary here.
- `table{border-collapse:separate!important}` — added to kill Blitz's phantom table borders. This one
  is **actively wrong** for a real engine: it would corrupt every email that legitimately collapses
  its borders. **It must be removed as part of the migration.**

## Gate 3 — can hostile mail do anything? **No — even with the sanitizer switched off.**

The decisive test. Blitz could not run scripts *because it was too limited to*. WebKit **can**, so
containment must be proven, not assumed. Hostile payloads were fed **straight past the ammonia
sanitizer** into the iframe.

Containment = `sandbox="allow-popups allow-popups-to-escape-sandbox"` (no `allow-scripts`, no
`allow-same-origin`) plus the CSP already emitted by `safehtml::document()`.

| Vector | Result |
|---|---|
| Inline `<script>` | **Inert** |
| `<img onerror=…>` | **Inert** |
| `<svg onload=…>` | **Inert** |
| `<body onload=…>` | **Inert** |
| Nested `<iframe src="https://…">` tracker | **Never fetched** |
| 1×1 remote tracking pixel | **Never fetched** |
| Remote `<link rel=stylesheet>` | **Never fetched** |
| CSS `url()` tracker | **Never fetched** |
| `<form action="https://evil…">` | Renders, **cannot submit** (`form-action 'none'`) |
| `javascript:` link | Cannot navigate |

Three layers — sanitizer, sandbox, CSP — each hold alone. This is a **stronger** posture than what
Blitz gave us, and it is enforced by a hardened engine instead of by a renderer's incidental gaps.

## Gate 4 — dependency policy. **Clean.**

Tauri v2's tree contains **no** `reqwest`, `hyper`, `isahc`, `surf`, `sentry`, or `opentelemetry`.
(`tauri-plugin-http` would pull `reqwest` — we simply never add it.) The `deny.toml` no-egress ban
survives intact, and gets *tighter*: "Load images" becomes a CSP relaxation rather than a Rust image
inliner, so `remoteimg.rs` and **`ureq` are deleted** and the app ends up with **no HTTP client at
all**.

## Gate 5 — cold start. **WASM is free; the webview is not.**

Milliseconds from process exec (release builds, median of 5). Both rows render an identical 3-pane
UI with a 300-row reactive list.

| Stage | Leptos/WASM | Vanilla JS |
|---|---|---|
| Webview created | 190 | 199 |
| **WebKit booted, HTML parsed** | **817** | **833** |
| Runtime entered (wasm fetched + compiled + instantiated) | 831 | 836 |
| 300-row list mounted | 869 | 842 |
| **First paint** | **1009** | **984** |

**WASM costs ~25 ms.** The 212 KB blob (73 KB gzipped) instantiated in **10 ms**; Leptos then built
300 reactive rows in 38 ms vs vanilla JS's 6 ms. Both are noise. The frontend stack is therefore
**not** a performance decision — Leptos was chosen on ethos (Rust end to end, zero npm).

**The webview is the cost.** ~630 ms of that second elapses *before a single line of our code runs* —
WebKitGTK spawning its web process. Unavoidable, and identical for any frontend.

Against the incumbent, measured the same way:

| | Time to painted UI |
|---|---|
| Slint (today) | **~275 ms** |
| Any webview | **~1000 ms** (window appears at ~145 ms, but **blank**) |

A **~700 ms regression**, once per launch, not per interaction. Accepted deliberately, with two
mitigations: paint a skeleton immediately so the window is never blank, and cap cold start at
**1200 ms** in CI (constitution P4).

## Verdict

Switch. Every gate passed. The cold-start regression is real and is the price of a reading pane that
actually works.
