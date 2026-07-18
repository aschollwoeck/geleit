# Packaging GeleitMail

## Linux (supported)
`packaging/package-linux.sh [version]` builds the release binary and assembles
`dist/geleit-<version>-linux-x86_64.tar.gz` (binary + `geleit.desktop` + README + LICENSE).

CI builds and attaches this tarball to the GitHub Release automatically when a `v*` tag is pushed
(`.github/workflows/release.yml`). Runtime needs the system libs the app links against (webkit2gtk
4.1, GTK 3, fontconfig, xcb/xkbcommon) — the same packages CI installs.

The HTML viewer is **X11-only** (the webview is embedded via X11 child-window APIs); on Wayland the
reading pane falls back to text.

## Auto-update signing (APP-7, ADR-0013) — one-time maintainer setup

The in-app updater installs only **signed** releases. The private signing key is a secret **you** hold;
it is deliberately not in the repo. Until you set it, releases still ship the manual tarball above —
the release job just skips the (signed) AppImage + `latest.json` updater artifacts.

**Set it up once:**

1. Install the CLI (`cargo install tauri-cli`), then `cargo tauri signer generate -w geleit-updater.key`.
   It prints a **public** key and writes the **private** key to that file. **Keep the private key safe —
   losing it means you can never sign updates again**, and users' apps will reject anything else.
2. Put the **public** key in `crates/geleit-app/tauri.conf.json → plugins.updater.pubkey`, replacing the
   dev key that ships in the repo. Commit it.
3. In the GitHub repo settings → Secrets → Actions, add:
   - `TAURI_SIGNING_PRIVATE_KEY` — the *contents* of `geleit-updater.key`.
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — the password you chose (omit/blank if none).

**Each release** (pushing a `v*` tag) then also builds a signed AppImage and a `latest.json`
(`packaging/make-latest-json.sh`) and attaches both to the Release. The app's configured endpoint
(`.../releases/latest/download/latest.json`) serves them. **Bump the version in the workspace
`Cargo.toml`** before tagging — `tauri.conf.json` inherits it (no hardcoded `version`), so the updater's
"is this newer?" comparison always uses the real crate version.

> **Validate the first signed release by hand** — the updater's end-to-end install path (download →
> verify → swap → relaunch) can only be exercised with a real signed artifact, which requires your key.
> The app side (check, no-user-data, signature-refusal of a bad artifact) was verified locally.

## macOS / Windows — not yet built (APP-5 follow-up)
These are intentionally absent from the release matrix because the app won't build/run on them yet:
- **HTML viewer:** uses `wry` + `webkit2gtk` + a GTK main-loop pump + X11 child-window embedding —
  all Linux-specific. Porting needs the platform webviews (WKWebView / WebView2) and a different
  embedding strategy.
- **Keychain:** secrets use the Linux Secret Service (`OsSecretStore`); macOS Keychain / Windows
  Credential Manager backends are pending (noted since M2).

Once the webview + keychain are abstracted per-platform, add `macos`/`windows` jobs to
`release.yml` with the corresponding packaging steps (`.app`/`.dmg`, MSI/zip).
