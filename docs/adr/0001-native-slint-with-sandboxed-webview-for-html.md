# ADR-0001: Native Slint UI with a sandboxed webview component for HTML email

## Status
Proposed — **both gates PASSED**: S0.2 (HTML rendering, see
`docs/technical/s0.2-html-spike-findings.md`) and S0.3 (virtualized 50k-row list, see
`docs/technical/s0.3-list-spike-findings.md`). Final confirmation in **S0.4**.

## Context
- Constitution P4: **native, not a webview shell.** Lean, low-RAM, native feel is the brand.
- But email is fundamentally a web document, and there is **no production-safe native Rust
  HTML/CSS renderer.** A mail client must render arbitrary, hostile HTML safely.
- UI stacks considered: **Tauri** (a webview *shell* — contradicts P4), **Slint** (native),
  with egui and Electron already rejected (see `GELEITMAIL.md`, `vision.md`).

## Decision
- The application shell is **Slint** (native Rust UI).
- HTML email is rendered by a **sandboxed webview *component*** embedded **only** in the
  message pane — never as the app shell. This is the P4 "native" carve-out: the app stays
  native; the web engine is contained to rendering one hostile document.
- The specific webview library and per-OS sandboxing/isolation mechanism are deferred to the
  S0.2 slice plan.

## Validation gates (M0 spikes)
This decision is confirmed only if both hold:
- **S0.2 (HTML): ✅ PASSED** — wry/WebKitGTK rendered the corpus; the sanitized adversarial
  email made **zero** outbound connections (raw leaked to 3 hosts), via pre-render sanitization.
  Caveats carried to M3/M4: the JS engine is on by default (must be explicitly disabled — wry has
  no toggle) and a CSS-aware sanitizer is needed for fidelity. Credible WKWebView/WebView2 path.
  Details: `docs/technical/s0.2-html-spike-findings.md`.
- **S0.3 (list): ✅ PASSED** (release build, full-list traversal) — Slint's virtualized
  `ListView` scrolls 50,000 rows at **64–65fps** (deep rows actually rendered; display vsync is
  ~75Hz), with memory cost equal to the row data alone (~335 B/row). The software fallback is
  near-smooth too (~57fps). Details: `docs/technical/s0.3-list-spike-findings.md`.

If a gate fails, revisit: fallback to embedding Servo/Verso, or a broader pivot.

## Consequences
- The engine stays UI-agnostic (guidelines §2); only the UI crate knows Slint / the webview.
- Cross-platform: keychain, HTML render host, and OAuth loopback sit behind
  platform-abstraction seams (roadmap S0.5).
- Guidelines §13 (UI conventions) is finalized once this is confirmed.
