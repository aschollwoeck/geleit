# GeleitMail

A **local-first, privacy-first** email client, written in Rust — a [Tauri](https://tauri.app) desktop
shell with a [Leptos](https://leptos.dev) (Rust→WASM) interface. **No middleman, no telemetry, no
tracking.**

Your mail is synced to your device and stored **encrypted at rest**; the app talks only to your own
provider's IMAP/SMTP servers. HTML mail renders in a sandbox with remote images blocked until you ask
for them.

## What it does

- **Read** — folders, a fast message list, conversation grouping, sandboxed HTML, offline.
- **Write** — compose / reply / reply-all / forward, Cc, attachments, Markdown formatting, per-account
  signature, drafts, address autocomplete.
- **Organize** — star, archive, delete→trash, move, empty trash, junk, create/rename/delete folders,
  multi-select bulk actions — all optimistic with server write-back.
- **Search** — instant, offline full-text search (in the encrypted index) with `from:` / `subject:` /
  `has:attachment` operators and across all accounts.
- **Multiple accounts**, keyboard navigation, and light/dark themes.

## Status

First release (**v0.1.0**) targets **Linux**. Sign-in is manual IMAP/SMTP today; one-click
Gmail/Outlook (OAuth) and macOS/Windows builds are planned. See the [CHANGELOG](CHANGELOG.md).

## Documentation

- **[User manual](docs/manual/README.md)** — how to use GeleitMail.
- [Security & privacy review](docs/security-review.md), [performance notes](docs/technical/performance-notes.md),
  and [architecture decisions](docs/adr/).

## Building (Linux)

Needs the system libraries the webview links against (webkit2gtk 4.1, GTK 3), the system tray library
(libayatana-appindicator3), and the
[`wasm-bindgen` CLI](https://crates.io/crates/wasm-bindgen-cli) (matching the version in `Cargo.lock`).

```sh
# Debian/Ubuntu system dependencies
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev
```

```sh
./scripts/build-ui.sh --release       # compile the Leptos frontend to WASM into dist/pkg/
cargo build --release -p geleit-app   # build the Tauri app (embeds dist/ at compile time)
GELEIT_DB="$HOME/geleit.db" ./target/release/geleit-app
```

`packaging/package-linux.sh` builds a release tarball; pushing a `v*` tag builds and attaches it to a
GitHub Release.

### Web version (self-hosted, localhost)

GeleitMail can also run as a local web app you open in a browser (ADR-0014). The engine runs in the
`geleit-server` process on your own machine; the browser talks HTTP to it. It binds `127.0.0.1` only,
so nothing else can reach it and your mail never leaves your hardware.

```sh
./scripts/build-ui.sh                  # build the WASM UI into dist/ (the server serves it)
cargo run -p geleit-server             # http://127.0.0.1:8080  (GELEIT_PORT / GELEIT_DB to override)
```

The web host needs none of the webview/tray system libraries above. Run the desktop app **or** the web
server against a given `GELEIT_DB`, not both at once (SQLite is single-writer).

By default it binds `127.0.0.1` only, so it's reachable from that machine alone and needs no login.

#### Reaching it across your network (opt-in)

To open GeleitMail from another device on your LAN (e.g. your phone), bind a non-loopback address and
set a password — the server **refuses to start** on a non-loopback bind without one, so your mailbox is
never served to the network unauthenticated:

```sh
GELEIT_BIND=0.0.0.0 GELEIT_PASSWORD='choose-a-strong-one' cargo run -p geleit-server
```

Every request then needs HTTP Basic auth (the browser prompts once; any username, that password).

**Put HTTPS in front.** Basic auth sends the password base64-encoded — safe over HTTPS, sniffable over
plain HTTP — so terminate TLS with a reverse proxy on the LAN. A whole [Caddy](https://caddyserver.com)
config is:

```
mail.example.lan {
    reverse_proxy 127.0.0.1:8080
}
```

Run `geleit-server` on `127.0.0.1:8080` (the default bind — no `GELEIT_BIND`) with `GELEIT_PASSWORD`
set, and let Caddy face the network; Caddy fetches/renews the cert and forwards to the app, which still
enforces the password. To reach it from *outside* your network, prefer a VPN or a tunnel (e.g.
[Tailscale](https://tailscale.com)) over port-forwarding — it's one shared password and single-user, not
a public service.

## Credits

The desktop shell is [Tauri](https://tauri.app) (Apache-2.0 / MIT) and the interface is
[Leptos](https://leptos.dev) (MIT) — Rust end to end. (The UI was Slint through M8; it moved to
Tauri + Leptos in M9, see [ADR-0012](docs/adr/0012-tauri-shell-with-leptos-ui.md).)

## License

MIT — see [LICENSE](LICENSE).
