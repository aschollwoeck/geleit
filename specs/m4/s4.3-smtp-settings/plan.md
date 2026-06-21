# S4.3 — SMTP account settings · Plan (the HOW)

- Store: migration #6 adds nullable smtp_host/smtp_port/smtp_security; `SmtpSecurityKind`
  (as_str/from_str → 'implicit'/'starttls'), `SmtpConfig`; `update_smtp_settings`/`smtp_settings`
  mirroring the IMAP ones. Username/password/self-signed flag stay shared with IMAP.
- App refresh: `build_smtp_settings` (pure, unit-tested); `run_setup` gains an `smtp: SmtpConfig`
  param, persisted via `update_smtp_settings` (rollback on new-account failure).
- App UI: form SMTP host/port fields + a STARTTLS checkbox (`f-smtp-*` props); `on_connect` builds +
  passes smtp; reconnect path pre-fills from `store.smtp_settings`.

## Verify
gates; store round-trip + build_smtp_settings unit tests; mutants (store) 0-missed; launch.
