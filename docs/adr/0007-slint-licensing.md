# ADR-0007: Slint under the Royalty-free license

## Status
Accepted (slice S1.7). Resolves the "Slint licensing tier" open decision from `GELEITMAIL.md`.

## Context
Slint is tri-licensed: `GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR commercial`. GeleitMail
is **MIT** (`LICENSE`). Using Slint under GPL-3.0 would force GeleitMail to become copyleft
(GPL-3.0), conflicting with the MIT choice; the commercial license is paid. The `cargo-deny` gate
(ADR-0002 / guidelines §6) flagged this when Slint was introduced in S1.7.

## Decision
Use Slint under its **Royalty-free license** (`LicenseRef-Slint-Royalty-free-2.0`). Our own code
stays MIT; Slint is consumed under its royalty-free desktop terms.

- `cargo-deny` allows **only** `LicenseRef-Slint-Royalty-free-2.0` (not `GPL-3.0-only`) for the
  Slint crates, so the build fails if the copyleft path is ever taken accidentally.
- **Attribution obligation:** the royalty-free license requires acknowledging Slint. Satisfied now
  in the project documentation (`README.md`); an in-app "About / credits" attribution is a
  **release requirement (M8, with APP-4 settings/About)** — tracked.

## Consequences
- GeleitMail ships free, stays MIT for our code, with a small attribution obligation.
- If a future need arises (e.g. removing attribution, or terms that don't fit), the commercial
  license is the escape hatch — revisit then.
- The Boost license (`BSL-1.0`) of a Slint transitive dep is permissive and was added to the
  allowlist alongside.
