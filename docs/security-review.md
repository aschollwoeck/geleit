# GeleitMail — Security & Privacy Review (M8 / S8.5, PRIV-5)

Date: 2026-06-22. Scope: the crypto, HTML-rendering, secrets, and network paths, and confirmation
of **no telemetry**. This is the first-release review; findings that were fixed during development
are noted as such.

## Privacy posture (honest framing)
GeleitMail is **local-first**: mail is fetched from, and sent to, **your own provider's servers** and
stored locally. We do **not** claim "mail never leaves your device" — it came from a server and goes
to one. What we *do* guarantee: **no middleman, no telemetry, no tracking**. The only network egress is
to the servers you configure, plus (only if you opt in per message) remote images in the viewer.

## Network egress — audited
- **Outbound connections in our code:** exactly one — `TcpStream::connect((imap_host, imap_port))` in
  `geleit-engine/src/imap.rs`, over TLS (rustls). SMTP goes through `lettre` (rustls) to the SMTP host
  you configure. Both targets are **user-supplied account settings**.
- **HTML viewer:** `wry`/webkit2gtk renders sanitized message HTML behind a CSP of `default-src 'none'`
  (S3.2). Remote content (images) is blocked by default and only loaded after an explicit per-message
  "Load remote images" (PRIV-2); a "remote content blocked" cue is shown (PRIV-3).
- **No HTTP client / telemetry SDK** is present in the dependency tree (verified: `reqwest`, `hyper`,
  `ureq`, `isahc`, `surf`, `sentry`, `opentelemetry` all absent). This is now **enforced in CI** via a
  `deny.toml` `[bans]` deny-list, so none can be introduced without a deliberate, reviewed change.

## Encryption at rest (SEC-1, ADR-0008)
- The local database is **SQLCipher** (AES). The 32-byte key is generated with a CSPRNG (`getrandom`)
  on first run and stored in the **OS keychain**, never in the DB or on disk in plaintext.
- Everything sensitive lives inside that encrypted DB: message bodies, **the full-text search index**
  (ADR-0010 — FTS5 chosen over an external engine precisely so the index isn't plaintext on disk),
  and **draft attachment blobs** (S4.15).
- Key handling refuses to overwrite a present-but-unreadable key (won't brick the DB on a transient
  keychain failure); a wrong key fails closed on first read.

## Secrets (SEC-2, P2)
- Account passwords + the DB key live in the OS keychain via the `SecretStore` seam
  (`OsSecretStore`, Secret Service on Linux). They are **never logged**: error types carry no
  credentials or message content, and `SmtpSettings`/`ImapConfig` have redacting `Debug`.

## HTML safety (M3)
- Two layers: `ammonia` sanitization (strips scripts/handlers, denies relative + scheme-relative
  URLs, strips remote `img`/`data:`/unsafe `href`) **and** the CSP network boundary. JavaScript is
  disabled in the webview. Fixes during dev: scheme-relative `//host` (→ `url_relative(Deny)`),
  `data:text/html` href phishing (→ external-open handler + href scheme allow-list).

## Insecure-TLS escape — build-gated
- `allow_invalid_certs` (for the local self-signed Dovecot in dev) is compiled **only** under the
  `dangerous-tls` feature, which is absent from release/CI builds; requesting it otherwise errors.

## Dependency hygiene
- `cargo deny` runs in CI: advisories, licenses (allow-list), sources, and the no-egress bans above.

## Residual risks / follow-ups (not release blockers)
- Read/flag state is **local-only** (no server `\Seen`/`\Flagged` write-back yet) — a correctness/sync
  gap, not a security one.
- macOS/Windows keychain backends + webview porting are pending (Linux is the supported platform).
- Trusted-sender persistence for remote images (currently per-message) is a future convenience.

## Conclusion
No telemetry, no tracking, no unexpected network egress; secrets and data encrypted at rest; HTML
sandboxed in depth. Suitable for a first (Linux) release on the privacy claims it makes.
