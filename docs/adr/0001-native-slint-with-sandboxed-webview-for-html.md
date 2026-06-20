# ADR-0001: Native Slint UI with a sandboxed webview component for HTML email

## Status
Proposed — to be confirmed by the M0 feasibility spikes (roadmap S0.2, S0.3).

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
- **S0.2 (HTML):** the sandboxed component renders a real-world HTML email corpus with the
  privacy invariant **zero outbound network requests** when remote content is blocked, **no
  script execution**, and a credible macOS/Windows sandboxing path.
- **S0.3 (list):** Slint renders ~50,000 message rows scrolling at **≥60fps with bounded
  memory**.

If a gate fails, revisit: fallback to embedding Servo/Verso, or a broader pivot.

## Consequences
- The engine stays UI-agnostic (guidelines §2); only the UI crate knows Slint / the webview.
- Cross-platform: keychain, HTML render host, and OAuth loopback sit behind
  platform-abstraction seams (roadmap S0.5).
- Guidelines §13 (UI conventions) is finalized once this is confirmed.
