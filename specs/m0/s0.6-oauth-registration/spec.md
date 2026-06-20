# S0.6 — OAuth app registration (kickoff) · Spec (the WHAT)

Slice of **M0** (`roadmap.md`). Type: **process / documentation** — no user stories, no code,
no end-user manual (guidelines §11). References ADR-0004 (OAuth seam), stories ACC-1/ACC-2.

Status: **draft.**

## Purpose
OAuth integration is M7, but Google and (especially) Microsoft app **verification has weeks of
lead time** (constitution/roadmap call this out). To avoid M7 being blocked, this slice produces
an **actionable registration guide + status tracker** so the registrations can be started now and
the review clock runs in parallel.

The actual registration is performed by the maintainer (it needs their Google/Microsoft accounts
and identity decisions); this slice cannot be done by automation.

## In scope
- A guide (`docs/technical/oauth-app-registration.md`) covering, for **Google (Gmail)** and
  **Microsoft (Outlook/M365)**:
  - app type = **desktop/native**, **loopback** redirect (RFC 8252), **PKCE**, no real secret;
  - the **scopes** needed for IMAP + SMTP access and refresh tokens;
  - the **verification/publishing** steps and their **expected lead times**;
  - where the resulting **client IDs** will live (app config at M7 — not in the repo).
- A **status tracker** table the maintainer fills in (started date, state) so progress is visible.

## Out of scope
- Performing the registrations (maintainer task, tracked here).
- Any OAuth code — the loopback/token flow is M7 (the `OAuthRedirect` seam already exists, S0.5).
- Committing any client IDs/secrets.

## Acceptance criteria
1. The guide lists concrete steps + scopes + redirect config for both providers.
2. Lead-time risks (Google restricted-scope verification; Microsoft publisher verification) are
   called out so they can be started early.
3. A status tracker exists for the maintainer to record progress.
4. No secrets/client IDs committed.

## Deliverables
- `docs/technical/oauth-app-registration.md` (guide + tracker).
- *(No code; no end-user manual.)*

## Note
This is the last M0 slice. Once the registrations are *started*, M0 is complete (the
registrations finishing is an external clock tracked in the guide, not a code dependency until M7).
