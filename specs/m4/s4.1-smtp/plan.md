# S4.1 — SMTP transport · Plan (the HOW)

## geleit-engine::smtp
- Deps: `lettre` (no-default, `tokio1`,`tokio1-rustls-tls`,`smtp-transport`,`builder`) — added.
- `pub struct SmtpSettings { host, port, username, security: SmtpSecurity, allow_invalid_certs }`.
- `pub enum SmtpSecurity { Implicit, StartTls, Plaintext }`.
- `pub async fn send(settings, password, envelope: &Envelope, message: &[u8]) -> Result<(),String>`:
  - builder per security: `relay(host)` (implicit TLS) / `starttls_relay(host)` / `builder_dangerous
    (host)` (plaintext, localhost only); `.port()`, `.credentials(Credentials::new(user,pass))`.
  - `allow_invalid_certs` (behind `dangerous-tls` feature): set `TlsParameters` with
    `dangerous_accept_invalid_certs(true)` for self-signed dev servers.
  - `transport.send_raw(envelope, message).await` → map errors to calm strings.
- Re-export `lettre::address::Envelope` (+ a small helper to build one from from/to strings) so the
  rest of M4 doesn't depend on lettre directly.

## Test (CI-runnable, no external server)
- A `#[tokio::test]` spins a `TcpListener` on `127.0.0.1:0`, accepts one connection, and speaks
  minimal SMTP (220 greeting; EHLO→250 + AUTH PLAIN; AUTH→235; MAIL FROM→250; RCPT TO→250;
  DATA→354 then read to `\r\n.\r\n`→250; QUIT→221), capturing MAIL/RCPT/credentials/body.
- `send` with `SmtpSecurity::Plaintext` to that port; assert captured envelope + body + auth.
- A second test: send to a closed port → `Err` (calm).

## Verify
gates; the in-process tests (CI); `.cargo/mutants.toml` excludes `smtp.rs`; `cargo deny` (lettre tree).
