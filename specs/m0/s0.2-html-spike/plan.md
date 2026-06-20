# S0.2 — Sandboxed HTML email rendering spike · Plan (the HOW)

Implements `spec.md`. References ADR-0001. Throwaway spike — code quality is not held to full
guidelines; it exists to produce **evidence**, not to ship.

## Location & isolation
- Harness lives in `spikes/s0.2-html-render/` — a standalone crate **excluded from the
  workspace** (`exclude = ["spikes/s0.2-html-render"]` in the root `Cargo.toml`). So
  `cargo build --workspace`, clippy, and CI never touch it; the shipping crates stay clean.
- A `spikes/README.md` marks the directory throwaway.

## Libraries
- **`wry`** — the embeddable webview component (ADR-0001). Linux backend = WebKitGTK (the libs
  installed for this slice). Same crate → WKWebView (macOS) / WebView2 (Windows).
- **`tao`** — windowing/event loop that pairs with `wry`.
- **`ammonia`** — HTML sanitizer (the primary safety mechanism; see below).

## Safety model (what actually enforces the gates)
Two layers; the spike measures both:
1. **Sanitization (primary):** before loading, run the email HTML through `ammonia` configured
   to **drop `<script>`, all `on*` handlers, `<link>`/remote `<style>`, and any remote-loading
   attribute** (img/src, srcset, background, `url(...)`), allowing only inline/`cid:`/`data:`
   content. After sanitization the document has **no remote references and no script**, so there
   is nothing to fetch or execute.
2. **Webview config (defense in depth):** disable JavaScript where `wry` exposes it; load via
   `with_html` (no base URL) so there is no implicit remote origin.

The spike proves the gate by **contrast**, using `strace`:
- **RAW** adversarial email (unsanitized, JS on) → expect remote `connect()` calls (tracker,
  script beacon) — demonstrates the threat is real.
- **SANITIZED** adversarial email → expect **zero** remote `connect()` — demonstrates mitigation.

`strace` catches both the privacy gate (remote images/pixels) and the no-script gate (a script
that beacons via `new Image().src=...` shows up as a `connect()` only when JS runs on raw input).

## Harness behavior (`src/main.rs`)
CLI: `spike <fixture.html> [--raw|--sanitize] [--title T]`.
- Read the fixture; if `--sanitize`, run it through the ammonia config above.
- Build a `tao` event loop + window (≈1000×800) on `$DISPLAY` (`:0`).
- Build a `wry` `WebView`, JS disabled if available, `.with_html(content)`.
- Render; after a short delay, exit (so the run is scriptable and the window does not linger).
- Print what it did (mode, byte counts, sanitized-out element counts) to stdout.

## Fixtures (`spikes/s0.2-html-render/fixtures/`, authored — no real mail)
- `newsletter.html` — tables, inline CSS, a remote image (fidelity sample).
- `receipt.html` — typical transactional layout.
- `multipart.html` — mixed content.
- `adversarial.html` — bundles: 1×1 tracking pixel (`http://`), remote image, remote
  `<link>` CSS, remote `@font-face`, inline `<script>` that does `new Image().src='http://…'`,
  an `onerror`/`onload` beacon, and a `javascript:` link.

## Evidence capture (`run-spike.sh` → `spikes/s0.2-html-render/evidence/`)
For each relevant run, launch the harness under:
`strace -f -e trace=connect -o evidence/<name>.strace <harness> <fixture> <mode>`
then post-process the strace log to count `connect()` calls to **non-loopback** AF_INET/AF_INET6
addresses. Record:
- `adversarial-raw.strace` (expect > 0 remote connects),
- `adversarial-sanitized.strace` (expect 0 remote connects),
- screenshots of the fidelity fixtures **if** a screenshot tool is available
  (`gnome-screenshot`/`scrot`/`import`); otherwise note the limitation (window did render).

## Verification (maps to acceptance criteria)
- AC2 privacy gate: `adversarial-sanitized.strace` shows **0** non-loopback connects (and raw
  shows > 0, proving the test is real).
- AC3 no-script gate: the script/`on*` beacons produce connects on raw but **none** on
  sanitized; sanitized output contains no `<script>`/`on*` (asserted by inspecting the
  sanitized HTML the harness prints).
- AC1 fidelity: screenshots (or documented render-without-error) per fixture.
- AC4: a written macOS/Windows note. AC5: findings recorded for ADR-0001 / S0.4.

## Findings report
`docs/technical/s0.2-html-spike-findings.md` — outcome, the strace evidence summary, whether
sanitization alone suffices or webview-level blocking is also needed, the cross-platform note,
and the ADR-0001 recommendation (confirm / amend).

## Risk / fallback
If `wry` cannot render acceptably or cannot be made safe even with sanitization, the fallback is
embedding Servo/Verso, or revisiting ADR-0001 — recorded as the spike outcome.
