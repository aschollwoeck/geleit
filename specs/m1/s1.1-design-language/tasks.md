# S1.1 — Visual design language · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md` (P8). Design/doc slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Do
- [x] Propose aesthetic directions (frontend-design skill) → maintainer chose **"Soft daylight"**
- [x] Write `design.md`: principles · layout/nav · typography · color/theme tokens (light+dark)
      · spacing/density/shape · components · iconography · motion · accessibility · voice
- [x] Cross-link guidelines §13 + stories (APP-2/3, READ-*, PRIV-3); M3 rich-content note

## Verify (acceptance criteria)
- [x] AC1 all nine areas with concrete usable values (tokens, type scale, light+dark palettes, spacing)
- [x] AC2 distinct/intentional + traces to constitution (review confirmed strong trace; sharpened
      distinctiveness with the "guiding edge" Geleit motif + warm "daylight" reading surface)
- [x] AC3 light + dark both specified
- [x] AC4 accessibility targets stated (and fixed: primary-button + danger text now AA)
- [x] AC5 buildable: S1.6/S1.7 can be implemented from it

## Ship
- [x] Code review (guidelines §11) — 1 agent: verified contrast math (caught primary buttons +
      danger text failing AA → fixed via accent-strong/danger-strong + dark-text-on-accent),
      defined missing `text-faint`, pinned privacy-chip surface, flagged generic signature →
      added the guiding-edge motif + warm reading surface
- [x] Update this tasks file to all-done
- [ ] PR merged (one-slice-one-PR, §12)
