# Plan — User manual + README

1. Audit shipped features vs existing docs → the manual had only `reading-mail.md`; everything from
   M4 (send), M5 (organize), M6 (search), M7 (multi-account), M8 (keyboard/theme/privacy) was undocumented.
2. Match the established manual voice (friendly, second-person, calm; honest privacy framing) by reading
   the existing `reading-mail.md`.
3. De-stale `reading-mail.md`: remove the "writing arrives shortly" banner, add SMTP + signature to the
   sign-in fields, replace the inline compose/remove sections with links to the new pages.
4. Write one page per feature area + a `docs/manual/README.md` index; cross-link them.
5. Rewrite top-level `README.md` as a landing page linking the manual, security review, perf notes, ADRs.
6. Verify accuracy against the actual UI (action names, shortcuts, operators) so nothing is documented
   that doesn't exist; keep honest "planned/not yet" notes (OAuth, macOS/Windows, save-to-disk).
7. Docs-only → CI is the gate (no code paths touched).
