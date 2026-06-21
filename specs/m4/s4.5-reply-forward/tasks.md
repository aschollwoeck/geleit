# S4.5 — Reply & Forward · Tasks

## Build
- [x] engine: Draft += in_reply_to/references; build() emits headers; reply()/forward()/Original + helpers
- [x] store: header_by_id
- [x] refresh: run_send carries in_reply_to + references
- [x] app: Reply/Forward links; pre-fill compose from the open message; compose_thread state

## Verify
- [x] AC1 build/test/clippy -D warnings/fmt/deny green
- [x] AC2 reply/forward/subject/quote/threading + header_by_id (tested)
- [~] AC3 Reply/Forward pre-fill + threaded send — MAINTAINER eyeballs live
- [x] AC4 mutants message + store 0-missed

## Ship
- [ ] tasks all-done; PR merged
