# S1.1 — Visual design language · Spec (the WHAT)

Slice of **M1** (`roadmap.md`), and the first UI-facing work. Type: **design / documentation** —
produces the canonical "what it looks like." No end-user manual; serves stories APP-2 (calm UI),
APP-3 (light/dark), and all READ-* views. Governed by `constitution.md`; complements
`guidelines.md` §13 (which is *how* it's implemented in Slint).

Status: **draft.**

## Purpose
Define GeleitMail's visual design language **before any UI is built** (Option A), so M1's read
view (S1.6/S1.7) and every later screen share one intentional, non-templated look that embodies
the brand: **calm, fast, native, private, for regular people** (constitution P3/P4/P5). Refined
for rich content (HTML, threads) in M3.

## Deliverable
A top-level **`design.md`** covering:
1. **Design principles** — the visual translation of calm/native/private/regular-people
   (e.g. quiet by default, content-first, generous but not wasteful space, no chrome noise).
2. **Layout & navigation** — the window structure (folder/account nav · message list · reading
   pane), responsive behavior, and where global actions live. Per-account *switcher* (MULTI-1),
   unified inbox later.
3. **Typography** — font choice(s) and a type scale (sizes/weights/line-height) for sender,
   subject, snippet, body, metadata.
4. **Color & theme tokens** — a small **semantic** token set (background, surface, text,
   muted, accent, unread, danger, divider…) with **light and dark** values (APP-3).
5. **Spacing & density** — a spacing scale and the default density (comfortable for regular
   people, not a power-user wall of text).
6. **Components** — the look of: message-list row (unread, attachment, date), reading-pane
   header, buttons, inputs, the "blocked N trackers" cue (PRIV-3), empty/loading states.
7. **Iconography** — style (weight, corner, fill vs line) and source/approach.
8. **Motion** — when/how things animate (subtle, calm; never blocking — P1/P3).
9. **Accessibility** — contrast targets, focus visibility, hit sizes, keyboard affordances.

## In scope
- The `design.md` document above, as **tokens + rules + ASCII/structural mockups** (not pixel
  comps). Concrete enough that M1's UI can be built to it.

## Out of scope
- Implementing it in Slint (S1.6+). Pixel-perfect mockups / a Figma file. Rich-content styling
  (HTML message body, threads) — refined in M3.
- Final icon assets (define the *style*; produce assets when building the UI).

## Acceptance criteria
1. `design.md` covers all nine areas with concrete, usable values (named tokens, a type scale,
   light+dark palettes, spacing scale).
2. The chosen direction is **distinct and intentional** (not a default), and traceably embodies
   the constitution (calm/native/private/regular-people).
3. Light **and** dark are both specified (APP-3).
4. Accessibility targets stated (contrast, focus, hit size).
5. It is buildable: M1's S1.6/S1.7 can be implemented directly from it.

## Deliverables
- `design.md` (top-level). *(No code; no end-user manual.)*
