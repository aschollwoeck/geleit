# Packaging GeleitMail

## Linux (supported)
`packaging/package-linux.sh [version]` builds the release binary and assembles
`dist/geleit-<version>-linux-x86_64.tar.gz` (binary + `geleit.desktop` + README + LICENSE).

CI builds and attaches this tarball to the GitHub Release automatically when a `v*` tag is pushed
(`.github/workflows/release.yml`). Runtime needs the system libs the app links against (webkit2gtk
4.1, GTK 3, fontconfig, xcb/xkbcommon) — the same packages CI installs.

The HTML viewer is **X11-only** (the webview is embedded via X11 child-window APIs); on Wayland the
reading pane falls back to text.

## macOS / Windows — not yet built (APP-5 follow-up)
These are intentionally absent from the release matrix because the app won't build/run on them yet:
- **HTML viewer:** uses `wry` + `webkit2gtk` + a GTK main-loop pump + X11 child-window embedding —
  all Linux-specific. Porting needs the platform webviews (WKWebView / WebView2) and a different
  embedding strategy.
- **Keychain:** secrets use the Linux Secret Service (`OsSecretStore`); macOS Keychain / Windows
  Credential Manager backends are pending (noted since M2).

Once the webview + keychain are abstracted per-platform, add `macos`/`windows` jobs to
`release.yml` with the corresponding packaging steps (`.app`/`.dmg`, MSI/zip).
