# S8.1 — Keyboard navigation + shortcuts (READ-9, APP-6) · Spec · start of M8

## In scope
- A global `FocusScope` placed BEHIND the UI with the window's focus forwarded to it. TouchAreas
  don't take keyboard focus, so shortcuts work everywhere except while typing in a field (which grabs
  focus, pausing them). Empty + full-size → no layout impact.
- Shortcuts: `j`/Down + `k`/Up move the list selection (and preview the message); `c` compose; `r`
  reply (when a message is open); `Esc` closes the open overlay or clears search. Disabled while an
  overlay/compose/setup is open (except Esc, which closes it).
- Pure `viewmodel::next_index` / `prev_index` (clamped, empty-safe) drive the movement; `nav-index`
  tracks the keyboard-focused row and resets on folder switch / search.

## Out of scope
- `/` to focus search (Slint can't focus a named element across `if`-scopes from here / from Rust);
  a visible focus ring distinct from selection; vim-style multi-key chords.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. `next_index`/`prev_index` tested + `cargo mutants` 0-missed.
3. j/k/c/r/Esc behave as described (maintainer eyeballs — keyboard focus is GUI-only).

## Deliverables
- `next_index`/`prev_index` + test; `keyscope` FocusScope + key-pressed; nav-index + nav handlers.
