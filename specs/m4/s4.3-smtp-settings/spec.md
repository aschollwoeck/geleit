# S4.3 — SMTP account settings · Spec (the WHAT)

Slice of **M4 (Send)**. Type: store + UI. Prerequisite for compose: the account stored only IMAP
settings, so there was no outgoing server to send through. Adds per-account **SMTP** settings
(host/port/security) to the model + the Add-account form, reusing the IMAP username/password.

Status: **draft.**

## In scope
- Store: migration #6 (`smtp_host`/`smtp_port`/`smtp_security`), `SmtpConfig` + `SmtpSecurityKind`
  (Implicit/StartTls), `update_smtp_settings` / `smtp_settings`.
- App: `build_smtp_settings(host, port, starttls)` (pure; default 465 implicit / 587 STARTTLS);
  `run_setup` persists SMTP; the Add-account form gains SMTP server/port + a STARTTLS toggle, and the
  reconnect path pre-fills them.

## Out of scope
- The compose window + actual send wiring (next slice). Separate SMTP username (reuses IMAP's).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. Store: SMTP settings round-trip + default-None; both security kinds persist — tested.
3. `build_smtp_settings`: port defaults per security; rejects empty host / bad port — tested.
4. `run_setup` saves SMTP (rolled back with the account on a new-account failure).
5. `cargo mutants` — store SMTP methods covered, 0 missed (build_smtp_settings is in the excluded
   refresh.rs, unit-tested).

## Deliverables
- Store migration + `SmtpConfig` + methods + tests; `build_smtp_settings` + form fields + reconnect
  pre-fill; tests.
