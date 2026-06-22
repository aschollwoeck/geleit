# GeleitMail

A native, **local-first, privacy-first** email client, written in Rust with a [Slint](https://slint.dev)
interface. **No middleman, no telemetry, no tracking.**

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

Needs the system libraries the UI links against (webkit2gtk 4.1, GTK 3, fontconfig, xcb/xkbcommon).

```sh
cargo build --release -p geleit-app
GELEIT_DB="$HOME/geleit.db" ./target/release/geleit-app
```

`packaging/package-linux.sh` builds a release tarball; pushing a `v*` tag builds and attaches it to a
GitHub Release.

## Credits

The user interface is built with [Slint](https://slint.dev), used under its Royalty-free license.

## License

MIT — see [LICENSE](LICENSE).
