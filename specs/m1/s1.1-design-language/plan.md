# S1.1 — Visual design language · Plan (the HOW)

Implements `spec.md`. Documentation/design — no code.

## Approach
Design is subjective and the maintainer's call, so we **converge on a direction first, then
flesh it out** (reacting to options is easier than specifying from scratch):

1. **Direction options.** Using the `frontend-design` skill for guidance (distinctive, not
   templated), propose **2–3 concrete aesthetic directions** for a calm/native/private mail
   client for regular people — each with a one-line ethos, a tiny palette, type feel, density,
   and an ASCII layout mockup. Maintainer picks one (or blends).
2. **Flesh out `design.md`.** Write the chosen direction into the nine sections from the spec:
   principles, layout/navigation, typography, color/theme tokens (light+dark), spacing/density,
   components, iconography, motion, accessibility — as **named semantic tokens + rules +
   structural ASCII mockups**, concrete enough to build M1's UI from.
3. **Cross-link.** Reference `guidelines.md` §13 (implementation) and the relevant stories
   (APP-2/3, READ-*); note rich-content refinement happens in M3.

## Layout starting point (to confirm with the direction)
Single-account M1: a **three-region** desktop layout — left nav (accounts/folders), center
message list, right (or below) reading pane — tuned for calm/regular-people density. Per-account
*switcher* now (MULTI-1); unified inbox later.

## Verification
- `design.md` satisfies all five acceptance criteria (nine areas, light+dark, a11y, buildable,
  intentional).
- Review = design + consistency pass (does it embody the constitution; is it internally
  consistent; can S1.6/S1.7 be built from it).
