# S4.11 — Attachments · Tasks
## Build
- [x] engine: Draft.attachments + Attachment + build() via mail-builder; guess_content_type + tests
- [x] refresh: run_send carries attachments
- [x] app: path field + Attach/Remove + display model; clear on new/reply/forward/resume
## Verify
- [x] AC1 build/test/clippy -D warnings (+ dangerous-tls)/fmt/deny green
- [x] AC2 attachment parse-back + guess_content_type every-arm (tested)
- [~] AC3 Attach/Remove + sent message carries file — MAINTAINER eyeballs
- [x] AC4 mutants message 0-missed
## Ship
- [ ] tasks all-done; PR merged
