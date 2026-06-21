# S3.6 — HTML fidelity + webview performance · Spec (the WHAT)

Slice of **M3** (reopened after maintainer review). Fixes the two things that made the shipped HTML
viewer feel broken: (a) rendered mail had **no formatting at all** (the sanitizer stripped every
style/class/link), and (b) **perceptible lag + a black flash** opening mail.

Status: **draft.**

## Purpose
Make HTML mail actually look like mail — colors, fonts, layout, working links — while keeping the
security guarantees, and make opening it feel instant (guidelines §5: every interaction instantaneous).

## In scope
- **Fidelity:** sanitizer keeps inline `style`, `<style>`, `class`, `<font>`, presentational attrs
  (`bgcolor`/`align`/`width`…), `http(s)`/`mailto` links, and `cid:`/`data:` inline images. Security
  shifts to a **layered model**: the **CSP is the network boundary** (blocks CSS `url()` remote
  loads + scripts), not aggressive HTML stripping. Remote `<img>` still blocked by default (PRIV-1).
- **Performance:** build the webview **once at startup** (hidden, pre-painted with the page bg) so
  the first mail open is instant and never flashes black.
- **Size:** `[profile.release]` strip + thin-LTO (≈460 M debug → **30 M** release).

## Out of scope
- Deep CSS parsing/property allowlisting (CSP covers the security need); trusted-sender persistence;
  Wayland embedding; save-attachments. Per-open render latency beyond the pre-build/pre-paint fix.

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. Sanitizer keeps style/`<style>`/class/font/presentational attrs + http links + cid:/data: images;
   strips scripts/`on*`/`javascript:` + remote `<img src>` — tested.
3. `document()` CSP unchanged (default-src 'none'; script blocked; img-src relaxed only on opt-in).
4. App builds the webview at startup; first mail open shows formatted content without a black flash
   (maintainer eyeballs).
5. `cargo mutants` — sanitizers covered; 0 missed. Release binary ≈30 M.

## Deliverables
- Rewritten `safehtml` (fidelity + layered model) + tests; eager webview build; release profile;
  manual update. Closes the maintainer's #2–#5 feedback.
