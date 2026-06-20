# S0.2 — Sandboxed HTML email rendering spike · Spec (the WHAT)

Slice of **M0** (`roadmap.md`). Type: **throwaway feasibility spike** — no user stories, no
end-user manual; success is measured pass/fail criteria (guidelines §11). Spike code is
**throwaway** and must be rewritten to full guidelines before any of it enters a real milestone.
References: ADR-0001 (native Slint + sandboxed webview component for HTML).

Status: **draft.**

---

## Purpose — the existential risk

GeleitMail is native (constitution P4), but email is arbitrary, hostile HTML and there is no
safe native Rust HTML renderer. ADR-0001 commits to rendering HTML email in a **sandboxed
webview component**. This spike proves that approach is actually feasible — specifically that we
can render real-world HTML email **without leaking anything to the network** and **without
executing hostile code**. If we cannot, the whole UI plan changes, so we find out now.

## What the spike must demonstrate

1. A webview component can **render real-world HTML email** with acceptable fidelity.
2. **The privacy gate (the hard invariant):** rendering an email that contains remote content
   (tracking pixel, remote images, remote CSS/fonts) makes **zero outbound network requests**.
3. **No script execution:** inline `<script>`, `on*` handlers, and `javascript:` URLs do nothing.
4. The approach has a **credible cross-platform path** (Linux now; macOS/Windows identified).

## In scope

- A minimal throwaway harness that loads an HTML email string into a sandboxed webview and
  displays it (standalone — does **not** need to be wired into `geleit-app`/Slint yet).
- A small **corpus of representative HTML emails** authored for the spike (no real personal
  mail — guidelines §11): a newsletter (tables, inline CSS, remote images), a receipt, a
  multipart-style message, and an **adversarial** sample bundling a tracking pixel, remote
  image, remote CSS/font, inline `<script>`, an `on*` handler, and a `javascript:` link.
- A **network-observation method** that can prove zero outbound requests (e.g. tracing
  `connect()` syscalls to non-loopback addresses, or routing egress to a logging sink).
- A short **findings report** that either confirms ADR-0001 or recommends a change.

## Out of scope

- Integration into the Slint app / message pane (that is M3).
- The remote-content **opt-in** UX ("load remote content", trackers-blocked cue) — M3/M4.
- Production-grade sandboxing hardening and the sandbox-escape test suite — M4 (S0.2 only needs
  a *credible* isolation approach demonstrated, not the final hardened one).
- Final HTML sanitization library/pipeline choices — only enough to pass the gates here.

## Acceptance criteria (measurable)

1. The harness renders every email in the corpus and produces a **screenshot per sample**
   (visual evidence of acceptable fidelity).
2. **Privacy gate:** while rendering the adversarial sample, the network-observation method
   records **zero outbound connections to any non-loopback address**. Demonstrated and captured.
3. **No-script gate:** the adversarial sample's script/`on*`/`javascript:` payloads have **no
   observable effect** (e.g. a payload that would alter the DOM or attempt a fetch does neither),
   and contribute zero network requests.
4. A written note identifies the **macOS (WKWebView) and Windows (WebView2)** path for the same
   approach, with known risks/unknowns.
5. **ADR-0001 outcome recorded:** confirmed, or amended with the evidence (this also feeds S0.4).

## Deliverables

- Throwaway spike harness (clearly marked; lives outside the shipping `crates/` — see plan).
- The HTML email corpus (committed fixtures) + captured screenshots.
- The network-observation script/method + its captured output (the zero-network evidence).
- Findings report in `docs/technical/` (or the slice dir) feeding ADR-0001 / S0.4.
- *(No end-user manual — spike slice.)*

## Open questions for the plan (`plan.md`)

1. **Webview library + Linux backend** (e.g. `wry` over WebKitGTK) and how it is acquired in
   CI / locally (system dev packages).
2. **How remote content is blocked** — pre-render HTML **sanitization** (strip/neutralize remote
   refs), webview **request interception** (deny non-loopback), or both (defense in depth).
3. **How JS is disabled** at the webview level.
4. **The concrete zero-network measurement** (strace `connect`, a deny-all egress sink, or a
   network namespace) and how its output is captured as evidence.
5. **Where the throwaway harness lives** (a `spikes/` dir excluded from the workspace, vs. an
   `examples/` target) so it never contaminates the shipping crates.
6. Whether/how any of this runs in **CI** (system webview deps make headless CI nontrivial — it
   may be a documented local-only spike).
