# S0.2 — Sandboxed HTML email rendering spike · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md`. Kept current (P8). Throwaway spike.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Build (throwaway harness)
- [x] Exclude `spikes/s0.2-html-render` from the workspace (root `Cargo.toml`) + `spikes/README.md`
- [x] Spike crate `Cargo.toml` (wry 0.55, tao 0.35, ammonia 4)
- [x] `src/main.rs` — load fixture, optional ammonia sanitize, render in wry, auto-exit, `--dump`
- [x] Fixtures: `newsletter.html`, `receipt.html`, `multipart.html`, `adversarial.html` (TEST-NET IP)
- [x] `run-spike.sh` — run under strace, count connect() to the remote host, capture evidence

## Verify (acceptance criteria — measurable)
- [x] AC1 all 4 fixtures render + clean exit (3 fidelity captured in `*.render.stderr`,
      adversarial under strace); sanitized newsletter HTML captured as fidelity evidence;
      screenshots omitted to avoid capturing the live desktop
- [x] AC2 privacy gate: sanitized adversarial → **0** connects; raw → **3** distinct hosts (real)
- [x] AC3 script behavior measured honestly via IPC oracle: JS engine on (both runs); email's
      inline `<script>` did not run; sanitized output `contains_script=false` (removal is the
      relied-upon control). Recommendation: explicitly disable JS in M3/M4
- [x] AC4 macOS (WKWebView) / Windows (WebView2) path noted with unknowns
- [x] AC5 ADR-0001 outcome recorded (S0.2 gate passed; S0.3/S0.4 pending)

## Document
- [x] `docs/technical/s0.2-html-spike-findings.md` — evidence + recommendation
- [x] ADR-0001 status updated (S0.2 PASSED; JS-disable + CSS-aware sanitizer flagged for M3/M4)
- [x] (No end-user manual — spike slice)

## Ship
- [x] Code review of the slice diff (guidelines §11) — addressed: added a positive control
      (IPC + per-vector hosts) for JS execution after the reviewer flagged the original
      "no script execution" claim as overclaimed; corrected findings/ADR to match evidence;
      captured render evidence for all 4 fixtures
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)
