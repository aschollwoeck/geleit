# ADR-0006: IMAP / TLS transport stack

## Status
Accepted (slice S1.4).

## Context
M1 needs to connect to IMAP servers. Constitution §5 requires all network I/O to be async (the
UI never blocks, P1). We must choose an async IMAP client and a TLS stack that builds in this
environment (no system OpenSSL dev headers; no `cmake`).

## Decision
- **Runtime:** `tokio`.
- **IMAP:** `async-imap` with the `runtime-tokio` feature (so it speaks tokio's I/O traits
  directly — the tokio-rustls stream is passed straight in, no futures/compat shim).
- **TLS:** `rustls` with the **`ring`** crypto provider, via `tokio-rustls`. CA roots from
  `webpki-roots` for the verified path.
- **Dev-only escape hatch:** an `allow_invalid_certs` flag selects a custom `ServerCertVerifier`
  that skips certificate-chain validation (accepts the local self-signed Dovecot cert) but still
  verifies handshake signatures. Off by default; never for real providers.

### Why this stack
- System OpenSSL dev headers are absent, so `native-tls` would need a vendored OpenSSL build →
  `rustls` (pure Rust) is cleaner and aligns with the integrity ethos.
- `rustls`' default `aws-lc-rs` provider needs `cmake` (absent); `ring` builds with `cc`+`perl`
  (present), so `ring` is the provider.

## Consequences
- Credentials are fetched through the platform `SecretStore` seam (SEC-2, ADR-0004) and never
  logged (P2). The **real OS-keychain backend** for `SecretStore` is still deferred (in-memory
  double in tests); a dedicated follow-up adds it.
- The same TLS stack will back SMTP (M5) and is the basis for provider OAuth (M7).
- Live IMAP behavior is verified by `#[ignore]`d integration tests against a local Dovecot
  (`geleittest@127.0.0.1:993`, `allow_invalid_certs`), run with `cargo test -- --ignored`; CI
  can't reach a server, so they're skipped there (guidelines §7). The network module is excluded
  from mutation testing for the same reason (`.cargo/mutants.toml`).
- `cargo-deny` allowlist gained `Zlib` (foldhash) earlier and **`CDLA-Permissive-2.0`**
  (webpki-roots' Mozilla CA *data*) here — both permissive, added deliberately as the gate flagged them.
