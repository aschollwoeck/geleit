# ADR-0009: SMTP send + message-building stack

## Status
Accepted (slice S4.1).

## Context
M4 sends mail. Constitution §5 requires async network I/O (P1), and ADR-0006 already committed the
project to **rustls/ring** on the wire (no OpenSSL — the build environment has no system OpenSSL dev
headers). We need an async SMTP client over that same TLS stack, plus a way to build correct RFC 5322
/ MIME message bytes.

## Decision
- **SMTP transport:** `lettre` (`AsyncSmtpTransport<Tokio1Executor>`), `default-features = false`
  with `tokio1`, `tokio1-rustls-tls`, `smtp-transport`, `builder`. So SMTP runs on **rustls** —
  symmetric with the IMAP stack (ADR-0006), no OpenSSL on the wire.
- **Message building:** `mail-builder` (sibling of the `mail-parser` already used for reading) builds
  the RFC 5322 bytes; we hand those to lettre via `AsyncTransport::send_raw(&Envelope, &[u8])`. This
  keeps message construction independent of the transport (the engine owns the bytes; lettre only
  speaks the protocol). *(Message building lands in S4.2; S4.1 is transport only.)*
- **Security modes:** implicit TLS (465), STARTTLS (587), and plaintext (localhost only, for tests).
- **Dev-only escape hatch:** an `allow_invalid_certs` flag (behind the engine `dangerous-tls`
  feature, as with IMAP) accepts a self-signed dev server's cert. Off by default; never for real
  providers.

## Why this stack
- `lettre` is the de-facto async SMTP client for Rust, supports rustls directly, and handles the SMTP
  state machine (EHLO/AUTH/STARTTLS/pipelining) we'd otherwise hand-roll and get subtly wrong.
- `send_raw` lets us own the MIME bytes (built by `mail-builder`), so the transport choice and the
  message format are decoupled and independently testable.
- `mail-builder` matches `mail-parser` (same author, same MIME model) — consistent encoding/decoding.

## Consequences
- Credentials flow through the platform `SecretStore` seam (ADR-0004) and are never logged (P2),
  reusing the IMAP password storage.
- **Transport is verified by a self-contained in-process SMTP sink** (a tokio TCP listener speaking
  minimal SMTP) — unlike the `#[ignore]`d live IMAP tests, this runs in **CI** (plaintext localhost,
  no TLS, no external server). The `smtp` module is still excluded from mutation testing where it is
  pure I/O glue (`.cargo/mutants.toml`), consistent with `imap.rs`.
- `cargo-deny` may need new permissive licenses for lettre's dependency tree; added deliberately as
  the gate flags them.
