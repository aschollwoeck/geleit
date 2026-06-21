# S4.1 — SMTP transport · Spec (the WHAT)

First slice of **M4 (Send)**. Type: engine. Delivers the core ability to **hand a built message to
an SMTP server and have it accepted** — the foundation for compose/reply/forward (SEND-1…3) and
Sent-folder/outbox. Message *building* (mail-builder) is S4.2; this slice is transport only.

Status: **draft.**

## Purpose
A reliable, async, rustls-backed SMTP send (ADR-0009) the rest of M4 builds on, verified end-to-end
without an external server.

## In scope
- `geleit-engine::smtp`: `SmtpSettings` (host/port/username/security/allow_invalid_certs),
  `SmtpSecurity` (Implicit 465 / StartTls 587 / Plaintext), and
  `async fn send(settings, password, envelope, message_bytes) -> Result<(), String>` over
  `lettre` + rustls. Password via the `SecretStore`-fetched value (caller passes it).
- Errors are calm, PII-free strings (P2).
- A self-contained **in-process SMTP sink** test (CI-runnable) proving a message is delivered with
  the right envelope + bytes, incl. AUTH.

## Out of scope
- Message/MIME building (S4.2). Compose UI (S4.2+). Sent-folder APPEND (later M4). Outbox/retry
  (later M4). OAuth (M7).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. `send` delivers to a local plaintext SMTP sink: server receives the exact MAIL FROM / RCPT TO
   envelope and message bytes; AUTH PLAIN credentials are presented — asserted by an in-process test
   that **runs in CI** (no `#[ignore]`, no external server).
3. A bad server / refused recipient surfaces a calm `Err(String)` (no panic, no PII).
4. P1/P2 honoured: async; credentials + addresses never logged.
5. `cargo mutants` — any pure helpers covered; the I/O `send` excluded like `imap.rs` (ADR-0009).

## Deliverables
- `engine::smtp` + the in-process-sink test; ADR-0009; mutants exclusion.
