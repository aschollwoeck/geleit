# S4.4 — Compose window + send · Tasks

## Build
- [x] engine `imap::password` getter (SMTP reuses the stored credential)
- [x] `refresh::run_send` (load account/SMTP/password → build → send, worker) + `parse_addrs`
- [x] app: "New message" button + compose overlay (To/Cc/Subject/Body/Send/Cancel/status)
- [x] handlers: on_compose (hide webview, clear), on_cancel_compose, on_send_message (worker)

## Verify
- [x] AC1 build/test/clippy -D warnings/fmt/deny green
- [x] AC2 parse_addrs tested (smtp/message CI-tested from S4.1/S4.2; run_send is live glue)
- [~] AC3 compose opens/validates/sends; calm error + sent note — MAINTAINER eyeballs + sends real
- [x] AC4 P1 (send off UI thread) / P2 (no password/address logging)

## Ship
- [ ] tasks all-done; PR merged
