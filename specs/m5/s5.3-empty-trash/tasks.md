# S5.3 — Empty trash / delete permanently · Tasks
## Build
- [x] engine: delete_permanently + empty_folder; drain pins the (!Unpin) expunge stream
- [x] refresh: run_delete_permanently + run_empty_folder + first_account_imap helper
- [x] app: viewing-trash (folder-selected); Empty Trash button; Delete→permanent in Trash
## Verify
- [x] AC1 build/test/clippy(+dangerous-tls)/fmt/deny green
- [~] AC2 permanent delete + empty trash — MAINTAINER eyeballs (engine ops live-tested)
## Ship
- [ ] tasks all-done; PR merged
