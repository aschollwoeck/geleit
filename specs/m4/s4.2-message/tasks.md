# S4.2 — Message building · Tasks

## Build
- [x] mail-builder dep; engine `message`: Draft + build() + recipients()
- [x] validation (sender + ≥1 recipient → calm error)
- [x] e2e test: build → smtp::send → in-process sink

## Verify
- [x] AC1 build/test/clippy/fmt/deny green
- [x] AC2 well-formed + parser round-trip + rejects missing sender/recipients (tested)
- [x] AC3 e2e delivers To+Cc + subject/body (CI)
- [x] AC4 mutants 0 missed (message)

## Ship
- [x] tasks all-done; PR merged
