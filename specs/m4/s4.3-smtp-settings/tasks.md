# S4.3 — SMTP account settings · Tasks
## Build
- [x] store: migration #6 + SmtpConfig/SmtpSecurityKind + update/get + tests
- [x] refresh: build_smtp_settings + run_setup persists smtp (+ rollback) + tests
- [x] app form: SMTP host/port + STARTTLS toggle; on_connect wiring; reconnect pre-fill
## Verify
- [x] AC1 build/test/clippy/fmt/deny green
- [x] AC2 store SMTP round-trip + default-None + both kinds (tested)
- [x] AC3 build_smtp_settings defaults + rejects (tested)
- [x] AC4 run_setup saves SMTP (rollback on failure)
- [x] AC5 mutants store 0-missed
## Ship
- [x] tasks all-done; PR merged
