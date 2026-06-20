# OAuth app registration guide & tracker

Register the Google and Microsoft OAuth apps **now** (M0 slice S0.6) so their verification clocks
run in parallel — OAuth integration itself is M7 (stories ACC-1 / ACC-2). The registration is a
maintainer task (needs your accounts); this doc is the checklist and the status tracker.

> **Verify against current docs.** Provider consoles, menu names, and scope strings drift. Treat
> the steps below as a map; confirm specifics against the live Google / Microsoft documentation
> when you register. Names/scopes here reflect the desktop IMAP+SMTP OAuth flow as of 2026-06.

## Desktop-OAuth model (both providers)
GeleitMail is a **native/desktop public client** (RFC 8252):
- **Redirect:** loopback — `http://127.0.0.1:<ephemeral-port>` / `http://localhost` (no custom
  scheme needed). This matches the `OAuthRedirect` seam (ADR-0004).
- **PKCE** is mandatory; there is **no confidential client secret** in a distributed binary.
- **`offline_access` / refresh tokens** so the client can re-auth without re-prompting (ACC-8).
- Resulting **client IDs** live in app config at M7 — **never commit client IDs or secrets.**

---

## Google (Gmail)
Console: `console.cloud.google.com`

1. **Create a project** (e.g. "GeleitMail").
2. **Enable the Gmail API** for the project.
3. **OAuth consent screen:** User type **External**; set app name, support email, developer
   contact, logo/domain as required.
4. **Scope:** `https://mail.google.com/` — full IMAP + SMTP access. ⚠️ This is a Google
   **restricted scope**.
5. **Create credentials → OAuth client ID → Application type: Desktop app.** Record the client ID.
6. Loopback redirect is supported for desktop clients (no manual redirect entry needed in most
   cases; confirm in console).

**⚠️ Lead-time risk (the slow part):** publishing an app that uses the restricted
`mail.google.com` scope requires **Google verification incl. a CASA security assessment**, which
can take **weeks** and may carry an **annual** re-assessment/cost. Meanwhile the app can stay in
**Testing** mode for up to ~100 explicitly-added test users — enough for development/beta. Start
verification early if a public release is the goal.

---

## Microsoft (Outlook / Microsoft 365)
Console: Microsoft Entra admin center — `entra.microsoft.com` → App registrations

1. **New registration.** Supported account types: **personal Microsoft accounts *and*
   work/school** (so Outlook.com consumers + M365 orgs both work).
2. **Authentication → Add a platform → Mobile and desktop applications**; add redirect
   `http://localhost`.
3. **Allow public client flows = Yes** (desktop/PKCE, no secret).
4. **API permissions → Office 365 Exchange Online → Delegated:**
   - `IMAP.AccessAsUser.All`
   - `SMTP.Send`
   - `offline_access` (refresh tokens)
   Record the **Application (client) ID**.
5. Note: some org tenants require **admin consent** for these permissions.

**⚠️ Lead-time risk:** to avoid the "unverified app" consent warning (and for broad distribution),
complete **publisher verification** (needs a verified Microsoft Partner/MPN account), and budget
for possible app review. Start this early.

---

## Status tracker (maintainer fills in)

| Provider | App name | Client ID obtained | Scopes set | Verification state | Started | Expected / cleared |
|---|---|---|---|---|---|---|
| Google (Gmail) | _tbd_ | ☐ | `mail.google.com` | Testing / In review / Verified | _yyyy-mm-dd_ | _tbd_ |
| Microsoft (Outlook) | _tbd_ | ☐ | IMAP.AccessAsUser.All, SMTP.Send, offline_access | Unverified / In review / Verified | _yyyy-mm-dd_ | _tbd_ |

Keep this table current. M7 (OAuth integration) needs the **client IDs** in hand; a public
release needs the **verification cleared** — start both clocks as early as possible.
