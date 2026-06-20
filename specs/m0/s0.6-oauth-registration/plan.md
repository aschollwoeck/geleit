# S0.6 — OAuth app registration (kickoff) · Plan (the HOW)

Implements `spec.md`. Documentation-only.

## Steps
1. Write `docs/technical/oauth-app-registration.md` with:
   - **Shared desktop-OAuth model:** native/desktop app, loopback redirect
     (`http://127.0.0.1:<ephemeral>` / `http://localhost`), **PKCE**, public client (the
     "secret" is not confidential). Matches the `OAuthRedirect` seam (ADR-0004).
   - **Google (Gmail) section:** Cloud Console project → enable Gmail API → OAuth consent screen
     (External) → **scope `https://mail.google.com/`** (IMAP+SMTP; a *restricted* scope) →
     create **Desktop** OAuth client → note client ID. Lead-time flag: restricted-scope
     verification (CASA security assessment) is the slow/expensive part; "Testing" mode works
     for ≤100 test users meanwhile.
   - **Microsoft (Outlook/M365) section:** Entra (Azure) → App registrations → New
     (supported accounts = personal + work/school) → platform **Mobile & desktop** with
     `http://localhost` redirect → **Allow public client flows** → API permissions (Office 365
     Exchange Online, delegated): **`IMAP.AccessAsUser.All`**, **`SMTP.Send`**, **`offline_access`**
     → note client ID. Lead-time flag: **publisher verification** (and possible app review) to
     avoid the unverified-app warning.
   - **Where client IDs live:** app config at M7, not in the repo; never commit secrets.
   - **Verify-against-current-docs caveat:** provider consoles/scope names drift; confirm against
     the live Google/Microsoft docs when registering.
   - A **status tracker** table (provider · app name · client ID obtained · scopes · verification
     state · started · expected/cleared) for the maintainer to fill in.
2. Keep accurate but provider-agnostic on exact URLs that rot (name the consoles:
   `console.cloud.google.com`, `entra.microsoft.com`).

## Verification
- Guide covers both providers with scopes, redirect, verification + lead times, and a tracker.
- No client IDs/secrets committed (none exist yet).
- Review = doc consistency pass (no code/tests for a process slice).
