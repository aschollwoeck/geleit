# S0.3 — Slint virtualized message-list spike · Spec (the WHAT)

Slice of **M0** (`roadmap.md`). Type: **throwaway feasibility spike** — no user stories, no
end-user manual; success is measured pass/fail (guidelines §11). Spike code is throwaway.
References: ADR-0001 (native Slint + sandboxed webview).

Status: **draft.**

---

## Purpose
ADR-0001 commits to **Slint** for the native UI. A mail client must scroll a large mailbox
smoothly (constitution P1/P3: calm + fast, instant). This spike proves Slint can render a
**virtualized message list of ~50,000 rows at ≥60fps with bounded memory** — the second of
ADR-0001's two gates (the first, HTML rendering, passed in S0.2). If it fails, the Slint commit
is revisited in S0.4.

## In scope
- A throwaway Slint harness showing a `ListView` bound to a model of ~50,000 message-like rows.
- A realistic row delegate: sender, subject, snippet, date, unread indicator, attachment marker.
- **Programmatic continuous scrolling** so the measurement reflects scrolling, not a static frame.
- **FPS measurement** (Slint's built-in performance counter) and **memory measurement** (max RSS).
- A **contrast run** at a small row count to show memory/perf do not scale with row count.
- A short findings report feeding ADR-0001 / S0.4.

## Out of scope
- Real message data, real visual design, theming, selection/interaction (M3).
- Wiring into `geleit-app` or the engine (M3).
- The HTML message pane (that was S0.2).

## Acceptance criteria (measurable)
1. The harness renders a **50,000-row** list with a realistic delegate and scrolls through it.
2. **≥60fps:** under continuous rendering + scrolling, Slint's performance counter reports a
   sustained frame rate **≥60fps** at 50k rows (captured to evidence).
3. **Bounded memory:** max RSS at 50k rows is modest and does **not** scale with row count like
   per-row widgets would — shown by comparing RSS at 1k vs 50k rows (captured to evidence).
4. ADR-0001 outcome recorded (S0.3 pass/fail), feeding S0.4.

## Deliverables
- Throwaway Slint harness in `spikes/s0.3-list-render/` (excluded from the workspace).
- `run-spike.sh` capturing FPS + max RSS for 1k and 50k rows into `evidence/`.
- Findings report in `docs/technical/`.
- *(No end-user manual — spike slice.)*

## Open questions for the plan (`plan.md`)
1. Slint version and whether to use the inline `slint!` macro vs a `.slint` file.
2. Exact FPS-capture method (Slint `SLINT_DEBUG_PERFORMANCE`) and how output is captured.
3. Memory measurement (`/usr/bin/time -v` max RSS) and the contrast row counts.
4. How scrolling is driven programmatically (animate the `ListView` viewport from a timer).
5. Renderer backend (GPU/femtovg vs software) and any fallback if GL is unavailable.
