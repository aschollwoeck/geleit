# Self-update (APP-7)

**Constitution:** P2 (privacy — nothing about the user leaves the machine), P3 (calm), P4 (lean),
P8 (spec-driven). **Amends** the "no HTTP client" posture — see the ADR + the exception below.
**Story:** APP-7 — have the app update itself.

## Why

A desktop app that can't update itself asks the user to babysit its releases — and, worse, leaves known
bugs and security fixes unshipped on machines that never re-download. Auto-update closes that gap: the
app notices a newer signed release and installs it.

## The tension, named — and the deliberate exception

M9 removed the app's HTTP client and `deny.toml` **bans `reqwest`/`ureq`/`hyper`**: today GeleitMail
talks to nothing but the user's own mail provider. Auto-update necessarily contacts a **release server**,
which reintroduces outbound HTTP. This is an **explicit, documented exception** (ADR-0013), narrowly
scoped:

- **The updater is the *only* non-IMAP/SMTP network the app makes**, and it talks to **one** endpoint: a
  static GitHub Releases feed. `reqwest` is un-banned in `deny.toml` **only** as `tauri-plugin-updater`'s
  transport, with a comment pinning it to that use.
- **No user data is ever sent.** The check is a GET of a static file. The request conveys only the app's
  **current version** and **platform/arch** (needed to serve the right binary) — plus the IP inherent to
  *any* network request, exactly as the existing IMAP/SMTP connections already reveal. **No mail, no
  addresses, no account info, no identifiers, no telemetry.** A static feed cannot collect anything, and
  the app sends nothing bespoke.
- **Tamper-proof.** Every update is **signed** (minisign) and the public key is compiled into the app;
  an update whose signature doesn't verify is refused. So even over the network, a MITM'd or forged
  update cannot install.
- **Opt-out.** An **"Automatically check for updates"** setting (default on) gates the on-launch check;
  off means the app only checks when the user presses **Check for updates**. Installing is **never
  silent** — the user always confirms *Install & restart*.

## Flow

`tauri-plugin-updater` (Tauri v2, official):

1. **Check** — on launch (after a short delay, if auto-check is on) and on the manual button:
   `app.updater()?.check()` GETs the feed and returns `Some(update)` when a newer, signed version exists.
2. **Surface** — the UI shows *"Update available: x.y.z"* with **Install & restart** (never auto-applied).
3. **Install** — `update.download_and_install()` downloads, **verifies the signature**, swaps the binary,
   and the app relaunches into the new version.

## App

- **Config** (`tauri.conf.json` → `plugins.updater`): `endpoints` = the GitHub Releases `latest.json`;
  `pubkey` = the release signing public key.
- **Setting** `auto_update` (bool, default `true`).
- **IPC**: `app_version()` → the running version; `check_update()` → `None` / `{version, notes}`;
  `install_update()` → downloads + installs the pending update, then relaunches.
- **UI**: Settings → **General** gains an *Updates* block — the current version, **Check for updates**
  with a status line (*Up to date* / *Checking…* / *Update available x.y.z → Install & restart* / a calm
  error), and the auto-check toggle.

## Release side (maintainer)

Real releases must be **signed with the maintainer's key** (the private half is a CI secret — I cannot
hold it). Documented in `packaging/README.md` + ADR-0013:

- `tauri signer generate` → keypair. Public key → `tauri.conf.json` `plugins.updater.pubkey`; private key
  + password → CI secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- `release.yml` builds the updater artifact (a **signed AppImage** on Linux — the format the updater can
  self-install; the plain tarball stays for manual installs), emits **`latest.json`**, and attaches both
  to the GitHub Release.

A **dev keypair** ships in this slice so the app compiles and the check flow is testable end to end
locally; the maintainer swaps in their own before the first signed release.

## Out of scope (named)

Delta/partial updates; a staging/rollback channel; update *content* other than the app (no data
migrations here — the store's own migrations handle that on next launch); macOS/Windows updater artifacts
(follow-ups once those platforms build, S8.4); telemetry of any kind (forbidden, PRIV-5).

## Acceptance criteria

1. `fmt` / `clippy -D warnings` / test / **`cargo deny check`** (with the scoped `reqwest` exception) /
   boundary all green; `perf-budget` unaffected (binary ceiling still met).
2. `check_update` against a **local feed** advertising a newer signed build reports it available, and one
   advertising the current version reports up-to-date — verified locally. (The version comparison itself
   is the plugin's semver check, so there's no bespoke comparison logic of ours to unit-test.)
4. No-user-data: the only outbound request is the feed GET; documented and reviewed. `cargo deny`'s
   allow-list confines `reqwest` to the updater.
5. The Settings *Updates* UI (version, check, status, toggle) works — maintainer's eyeball.
