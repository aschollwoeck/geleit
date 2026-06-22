# S7.4/S7.5 — Multiple accounts + switcher · Tasks
## Build
- [x] store: account_by_id + isolation test
- [x] refresh: account_id param on every worker; run_setup add-or-reconfigure-by-email returns id
- [x] app: current-account prop as source of truth; reload_all fills accounts + keeps current/first
- [x] app: rail switcher (other accounts + "+ Add account"); switch/add/cancel handlers
- [x] app: thread ui.get_current_account() into every worker call site; remove-account uses current
## Verify
- [x] AC1 build/test/clippy(+dangerous-tls)/fmt/deny green
- [x] AC2 account_by_id + isolation tested; store mutants 0-missed
- [~] AC3 add/switch/per-account mail/correct from-address/remove-fallback — MAINTAINER eyeballs
## Ship
- [ ] tasks all-done; PR merged
