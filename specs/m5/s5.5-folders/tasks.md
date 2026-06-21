# S5.5 — Create / rename / delete folders · Tasks
## Build
- [x] engine: create_folder/rename_folder/delete_folder; persist_folders prunes absent
- [x] store: prune_folders + test
- [x] refresh: run_create/rename/delete_folder (op + sync_folders reconcile) + folder_op helper
- [x] app: Manage folders… rail link + overlay (Create/Rename→/Delete) + handlers + spawn_folder_op
## Verify
- [x] AC1 build/test/clippy(+dangerous-tls)/fmt/deny green
- [x] AC2 prune_folders tested
- [~] AC3 create/rename/delete reflected in rail — MAINTAINER eyeballs
- [x] AC4 mutants store 0-missed
## Ship
- [ ] tasks all-done; PR merged
