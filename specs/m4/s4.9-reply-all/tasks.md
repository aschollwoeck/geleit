# S4.9 — Reply all · Tasks

## Build
- [x] store: migration #8 (to_addrs/cc_addrs) + capture in upsert + read in header_by_id + test
- [x] engine imap: join_addrs captures To/Cc on envelope sync
- [x] engine message: Original gains to/cc; reply_all() + split_addrs + tests
- [x] app: Reply all link + handler (open_compose kind: reply/reply-all/forward)

## Verify
- [x] AC1 build/test/clippy -D warnings (+ dangerous-tls)/fmt/deny green
- [x] AC2 to_addrs/cc_addrs round-trip (tested)
- [x] AC3 reply_all includes others, excludes me, dedups Cc vs To + itself (tested)
- [~] AC4 Reply-all pre-fills To+Cc — MAINTAINER eyeballs
- [x] AC5 mutants store + message 0-missed

## Ship
- [ ] tasks all-done; PR merged
