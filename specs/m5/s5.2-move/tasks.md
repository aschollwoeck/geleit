# S5.2 — Archive / Trash / Move · Tasks
## Build
- [x] engine: imap::move_message (UID MOVE)
- [x] store: delete_message
- [x] viewmodel: find_folder + test
- [x] refresh: run_move worker
- [x] app: Archive/Delete/Move… actions + Move-to picker + perform_move/remove_row/folder_names
## Verify
- [x] AC1 build/test/clippy(+dangerous-tls)/fmt/deny green
- [x] AC2 find_folder + delete_message tested
- [~] AC3 optimistic remove + write-back + restore-on-fail — MAINTAINER eyeballs
- [x] AC4 mutants store + viewmodel 0-missed
## Ship
- [ ] tasks all-done; PR merged
