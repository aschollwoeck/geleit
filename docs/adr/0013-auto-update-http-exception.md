# ADR-0013: Auto-update — the one sanctioned outbound-HTTP exception

## Status
**Accepted** (APP-7). Amends the "no HTTP client" posture established in **[ADR-0012](0012-tauri-shell-with-leptos-ui.md)**
(M9) and the `deny.toml` ban on `reqwest`/`hyper`/`ureq`. Does **not** touch **PRIV-5** (no telemetry) —
which this decision upholds explicitly.

## Context

M9 removed the app's HTTP client, and `deny.toml` bans every general HTTP client so none can be pulled
in unnoticed: today GeleitMail's only network is IMAP/SMTP to the user's own provider — "nothing about
your mail leaves this machine." That is a genuine privacy property, not a slogan.

But a desktop app that can't update itself (APP-7) strands security and bug fixes on machines that never
re-download, and asks every user to babysit releases. Auto-update **necessarily** contacts a release
server, which reintroduces outbound HTTP — directly against the M9 posture. The maintainer's call was:
**do the full auto-update, as an explicit, documented exception — provided no user data is ever sent.**

## Decision

Adopt **`tauri-plugin-updater`** (Tauri v2, official) as a single, narrowly-scoped exception:

1. **One client, one purpose.** `reqwest` (and its engine `hyper`) are un-banned in `deny.toml` **only**
   as the updater's transport, with comments pinning them to that use. They must never be used directly
   by our code; anything else pulling them in is a reviewed change that has to justify itself there.
   Every *other* HTTP client stays banned; all telemetry/analytics SDKs stay banned (PRIV-5).

2. **No user data — ever.** The check is a GET of **one static file** (a GitHub Releases `latest.json`).
   Because the endpoint is a plain URL with **no template variables** (`{{current_version}}`/`{{target}}`
   /`{{arch}}`), the request is a bare `GET /latest.json` — it carries **nothing app-specific at all**:
   the "is this newer?" comparison runs **on the device** after the feed is fetched. The only thing any
   network request inherently reveals is the IP, exactly as the existing IMAP/SMTP sockets already do. No
   version, no platform, no mail, no addresses, no account data, no identifiers, no telemetry. A static
   feed **cannot** collect anything. (Verified: a local run showed the outbound request is a bare
   `GET /latest.json`, no query string. *If* a templated endpoint were ever configured, version + platform
   would appear in the URL — still no user data — but the shipped config is static.)

3. **Tamper-proof.** Every update is **signed** (minisign); the public key is compiled into the binary,
   and an update whose signature doesn't verify is **refused**. So even over the network, a MITM'd or
   forged update cannot install. (Verified: an unsigned test artifact was correctly rejected.)

4. **Consent + opt-out.** On-launch checking is gated by an **"Automatically check for updates"** setting
   (default on; off = manual only). Installing is **never silent** — the user confirms *Install &
   restart* every time.

## Consequences

- **Positive:** users get security/bug fixes without manual re-downloads; the privacy property is
  *narrowed, not abandoned* — network egress is now "your provider, plus a signed, no-user-data update
  check you can turn off."
- **Cost:** `reqwest`/`hyper` re-enter the dependency tree (behind the updater); the binary grows; the
  release pipeline must sign artifacts.
- **The version the updater compares is the crate version** — `tauri.conf.json` no longer hardcodes
  `version`, so it can't drift from `Cargo.toml` and mislead the "is this newer?" check.

## Maintainer setup (required before the first signed release)

The private signing key is a secret the maintainer holds — it is deliberately **not** in the repo.

1. `cargo tauri signer generate` → a keypair. Put the **public** key in
   `tauri.conf.json → plugins.updater.pubkey` (a dev key ships now so the app compiles and the check
   flow is testable; replace it with yours). Store the **private** key + password as CI secrets
   `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
2. The release build produces a **signed AppImage** (Linux updater artifact — the plain tarball stays for
   manual installs) and a **`latest.json`**; both are attached to the GitHub Release. See
   `release.yml` + `packaging/README.md`.
3. Losing the private key means updates can no longer be signed — keep it safe.
