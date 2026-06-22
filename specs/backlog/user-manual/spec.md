# Backlog — User manual + README (documentation debt)

The manual only covered reading (`docs/manual/reading-mail.md`, partly stale) and the top-level
README was 5 lines — none of M4–M8's features (writing, organizing, search, multiple accounts,
keyboard/appearance, privacy) were documented for users.

## In scope
- De-stale `reading-mail.md` (drop "writing arrives shortly"; add SMTP/signature to sign-in; link out).
- New manual pages: `writing-mail.md`, `organizing-mail.md`, `searching-mail.md`, `accounts.md`,
  `shortcuts-and-appearance.md`, `privacy.md`, and a `docs/manual/README.md` index.
- Rewrite the top-level `README.md` into a real landing page (what it does, status, build, doc links).

## Out of scope
- Screenshots/GIFs; a hosted docs site; translating the manual.

## Acceptance criteria
1. Every shipped user-facing feature (M4–M8) is documented, accurately (no "coming soon" for things
   that exist; keep honest notes for what genuinely isn't built, e.g. save-attachments-to-disk, OAuth).
2. Calm, second-person prose matching the existing manual voice; pages cross-link; CI green.

## Deliverables
- 7 manual pages + index; rewritten README.
