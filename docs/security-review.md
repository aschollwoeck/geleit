# GeleitMail — Security & Privacy Review (M8 / S8.5, PRIV-5)

Date: 2026-06-22; **re-audited 2026-07-19** (whole-app pass + in-memory secret zeroization). Scope: the
crypto, HTML-rendering, secrets, custom-protocol/IPC, filesystem, SQL, and network paths, and
confirmation of **no telemetry**. Findings fixed during development are noted as such.

**2026-07-19 verdict:** the core guarantees — no-egress, no-script-in-mail, encrypted-at-rest,
no-injection, no cross-account leak — each hold, enforced by multiple independent layers; no
critical/high-severity break found. The only actionable item is the low-severity popup-routing check
noted under *HTML safety*.

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
- **In-memory lifetime is bounded (§9, `zeroize`).** The transient copies the app holds while working —
  the DB key returned by `db_key` and the hex/PRAGMA strings built from it (`open_encrypted`), the IMAP
  password at login (`connect`), and the SMTP password in `SendContext` — are `Zeroizing`, so they're
  wiped from the heap on drop rather than left in freed memory. (Copies inside third-party crates that
  take the key/password by value — SQLCipher's cipher context, lettre's SMTP `Credentials` — are outside
  our control; the wrapping clears every copy we own.)

## HTML safety (M3 / M9) — four independent layers
Mail HTML must clear **all four** before it could run anything (re-audited 2026-07-19, whole-app pass):
1. **`ammonia` sanitization** (over html5ever, which re-serializes — the recommended mXSS defence):
   strips `<script>`, `on*` handlers, `javascript:`/`vbscript:`, `<iframe>`/`<object>`/`<base>`/`<meta>`;
   `href` restricted to http(s)/mailto; remote `img` blocked by default, https-only on opt-in.
2. **Own origin, not `srcdoc`** — served from `mail://` so it carries its own CSP (a `srcdoc` frame would
   inherit the app's and strip the message's styles).
3. **CSP** (emitted both as a `mail://` response header and an in-page `<meta>`, with a test asserting the
   two are identical): `default-src 'none'` — **no `script-src` at all** — `form-action 'none'`,
   `base-uri 'none'`, `img-src`/`font-src` with no network host unless remote images are opted in.
4. **iframe sandbox** without `allow-scripts` or `allow-same-origin`, so even a CSP slip couldn't run
   script or reach the parent's IPC bridge.
- The privileged UI never renders mail HTML: the only `inner_html` sinks are compile-time SVG icon
  constants; the message body is never returned to the frontend as a string.
- **Noted follow-up (low):** the mail frame carries `allow-popups allow-popups-to-escape-sandbox` so a
  user-clicked `target=_blank` link surfaces as a new-window request the shell routes to the system
  browser (`navigation_action`). Since scripts are off, a popup is only ever user-initiated; worth a
  runtime test that a clicked link opens the *system* browser (not an in-app webview), and — if a popup
  can slip the `on_navigation` guard — an explicit new-window handler.
- CSS (inline/`<style>`/`url()`) is not parsed; the CSP is what blocks CSS-based tracking beacons — a
  deliberate trade-off with no second layer, so the `mailproto` CSP-parity test is load-bearing.

## Insecure-TLS escape — build-gated
- `allow_invalid_certs` (for the local self-signed Dovecot in dev) is compiled **only** under the
  `dangerous-tls` feature, which is absent from release/CI builds; requesting it otherwise errors.

## Dependency hygiene
- `cargo deny` runs in CI: advisories, licenses (allow-list), sources, and the no-egress bans above.

## Residual risks / follow-ups (not release blockers)
- macOS/Windows keychain backends + webview porting are pending (Linux is the supported platform).
- Trusted-sender persistence for remote images (currently per-message) is a future convenience — and a
  deliberate default: auto-loading remote content is the tracking-pixel exposure the per-message opt-in
  (PRIV-2) exists to avoid.
- Secret material copied *into* third-party crates by value (SQLCipher's cipher context, lettre's SMTP
  `Credentials`) can't be zeroized by us; our own transient copies are (see Secrets).

## Conclusion
No telemetry, no tracking, no unexpected network egress; secrets and data encrypted at rest; HTML
sandboxed in depth. Suitable for a first (Linux) release on the privacy claims it makes.
