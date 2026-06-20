# S0.6 — OAuth app registration (kickoff) · Tasks (done vs. to-do)

Derived from `spec.md` + `plan.md` (P8). Process/doc slice.
Status: `[ ]` todo · `[~]` in progress · `[x]` done.

## Do (this slice — the guide)
- [x] Write `docs/technical/oauth-app-registration.md` (desktop-OAuth model + Google + Microsoft
      steps, scopes, redirect, verification + lead-time flags, where client IDs live, tracker)
- [x] Verify-against-current-docs caveat included
- [x] No client IDs / secrets committed

## Verify (acceptance criteria)
- [x] AC1 concrete steps + scopes + redirect for both providers
- [x] AC2 lead-time risks called out (Google restricted-scope/CASA; Microsoft publisher verification)
- [x] AC3 status tracker present for the maintainer
- [x] AC4 no secrets/client IDs committed

## Ship
- [x] Review: doc consistency pass (process slice — no code/tests/manual)
- [x] Update this tasks file
- [ ] PR merged (one-slice-one-PR, §12)

## Maintainer follow-up (external clock — NOT part of this PR)
- [ ] Register Google (Gmail) desktop OAuth app; start restricted-scope verification
- [ ] Register Microsoft (Outlook) desktop OAuth app; start publisher verification
- [ ] Keep the tracker in `docs/technical/oauth-app-registration.md` current; client IDs needed by M7
