# S4.7 — Per-account signature · Tasks

## Build
- [x] store: migration #7 (signature) + update_signature/signature + round-trip test
- [x] engine: message::signature_block + test
- [x] refresh: run_setup persists signature
- [x] app: Signature form field + on_connect wiring + reconnect pre-fill; compose/reply/forward append

## Verify
- [x] AC1 build/test/clippy -D warnings/fmt/deny green
- [x] AC2 store signature round-trip + clear (tested)
- [x] AC3 signature_block delimiter + blank (tested)
- [~] AC4 new/reply/forward bodies include signature — MAINTAINER eyeballs
- [x] AC5 mutants store + message 0-missed

## Ship
- [ ] tasks all-done; PR merged
