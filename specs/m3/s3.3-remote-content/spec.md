# S3.3 — Load remote content + trackers-blocked cue · Spec (the WHAT)

Slice of **M3** (last). Type: engine + UI. Delivers **PRIV-3** (a calm "remote content blocked" cue
when an HTML message had remote refs stripped) and **PRIV-2** (a per-message "Load remote images"
opt-in that re-renders with remote content allowed). Builds on S3.1/S3.2.

Status: **draft.**

## Purpose
By default remote images/trackers are blocked (S3.1). When a message *had* such content, tell the
person ("Remote content blocked") and let them choose to **load it for this message** — a deliberate,
per-message opt-in (no silent loading, no tracking by default).

## In scope
- `geleit-engine::safehtml::sanitize_html_allowing_remote(raw)` — keeps `http(s)` refs (still strips
  scripts/handlers, PRIV-4). Pure, tested.
- App: when an open HTML message's blocked vs allowed sanitization differ (→ it had remote content),
  show a cue + **Load remote images** button; clicking re-renders the webview with remote allowed.

## Out of scope
- **Trusted-sender** persistence ("always load for this sender") — follow-up (needs a trusted-senders
  store). CSS-aware fidelity; Wayland.

## Acceptance criteria (measurable)
1. build/test/clippy/fmt/`cargo deny check` green.
2. `sanitize_html_allowing_remote` keeps `http(s)` img/links but still strips `<script>`/`on*` — tested.
3. App: HTML message with remote content shows the cue; "Load remote images" re-renders with remote;
   message without remote shows no cue. (Visual eyeballed by maintainer.)
4. P1/P2 unchanged; remote loads ONLY on explicit opt-in (no default network on read).
5. `cargo mutants` — both sanitizers covered; main.rs excluded; 0 missed.

## Deliverables
- `sanitize_html_allowing_remote` + tests; cue + load-remote UI; manual touch. **Completes M3.**
