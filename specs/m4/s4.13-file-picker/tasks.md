# S4.13 — Native file picker · Tasks
## Build
- [x] pick_file_via_dialog (zenity → kdialog subprocess; cancel/absent → None)
- [x] Browse… button + on_browse_file worker → fills path + invokes attach
## Verify
- [x] AC1 build/test/clippy/fmt/deny green (no new deps)
- [~] AC2 Browse opens chooser, attaches; cancel/absent graceful — MAINTAINER eyeballs
- [x] AC3 chooser runs on a worker (P1)
## Ship
- [ ] tasks all-done; PR merged
