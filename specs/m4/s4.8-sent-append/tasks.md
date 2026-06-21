# S4.8 — Save sent mail to Sent · Tasks

## Build
- [x] engine: imap::append_message (IMAP APPEND, \Seen) + live #[ignore] test
- [x] refresh: run_send finds the Sent folder + best-effort append after a successful send

## Verify
- [x] AC1 build/test/clippy -D warnings (+ dangerous-tls)/fmt/deny green
- [~] AC2 append_message does an APPEND — live #[ignore] test (MAINTAINER runs vs Dovecot)
- [x] AC3 run_send saves to Sent on success; a Sent-save failure never fails the send

## Ship
- [ ] tasks all-done; PR merged
