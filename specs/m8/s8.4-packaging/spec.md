# S8.4 — Cross-platform builds + installers (APP-5) · Spec

## Delivered: Linux
- `packaging/package-linux.sh` builds the release binary + assembles
  `dist/geleit-<version>-linux-x86_64.tar.gz` (binary + `.desktop` + README + LICENSE).
- `.github/workflows/release.yml`: on a `v*` tag, builds on Ubuntu and attaches the tarball to the
  GitHub Release (with generated notes). `dist/` is gitignored.

## Blocked: macOS / Windows (documented in packaging/README.md)
The app is Linux-only today: the HTML viewer uses `wry`/`webkit2gtk` + a GTK loop + X11 child-window
embedding, and secrets use the Linux Secret Service. macOS/Windows need platform webviews
(WKWebView/WebView2) + keychain backends first — a porting effort, not a packaging one, and not
testable in this environment. The release matrix is scaffolded to add those jobs later.

## Acceptance criteria
1. The Linux package script produces a tarball (verified locally → 11 MB). `release.yml` is valid.
2. Cross-platform status documented honestly.

## Deliverables
- `packaging/{package-linux.sh,geleit.desktop,README.md}`; `release.yml`; `.gitignore` dist/.
