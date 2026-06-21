# S4.1 — SMTP transport · Tasks

## Build
- [x] engine `smtp` module: SmtpSettings/SmtpSecurity + `send()` via lettre (rustls), Envelope helper
- [x] dangerous-tls path for self-signed dev servers (feature-gated)
- [x] ADR-0009; `.cargo/mutants.toml` excludes smtp.rs

## Verify
- [x] AC1 build/test/clippy/fmt/deny green
- [x] AC2 in-process SMTP sink test (CI): envelope + body + AUTH delivered
- [x] AC3 bad server → calm Err (no panic/PII)
- [x] AC4 async; no credential/address logging
- [x] AC5 mutants: helpers covered, smtp.rs excluded

## Ship
- [x] Code review (focus: TLS/security defaults, error PII, the test's correctness)
- [x] tasks all-done; PR merged
