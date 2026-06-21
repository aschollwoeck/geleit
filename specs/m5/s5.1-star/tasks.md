# S5.1 — Star / flag · Tasks
## Build
- [x] store: NewMessage.flagged + upsert (insert sets, re-sync preserves) + MessageHeader.flagged + set_flagged + tests
- [x] engine: imap::set_flag (UID STORE FLAGS) + drain helper
- [x] refresh: run_set_flag worker
- [x] app: viewmodel.starred; MessageItem.starred + r-starred; ★ toggle + list marker; on_toggle_star
## Verify
- [x] AC1 build/test/clippy(+dangerous-tls)/fmt/deny green
- [x] AC2 store flagged sync/preserve/set + message_vm.starred tested
- [~] AC3 ★ optimistic + write-back + failure note — MAINTAINER eyeballs
- [x] AC4 mutants store + viewmodel 0-missed
## Ship
- [ ] tasks all-done; PR merged
