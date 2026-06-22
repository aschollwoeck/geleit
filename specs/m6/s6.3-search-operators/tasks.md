# S6.3 — Search operators · Tasks
## Build
- [x] store: parse_search + ParsedSearch (from:/subject:/has:attachment) replacing fts_query
- [x] store: search_messages handles FTS / filter-only / empty; header_from_row helper
- [x] app: search placeholder hints operators
## Verify
- [x] AC1 build/test/clippy(+dangerous-tls)/fmt/deny green
- [x] AC2 parse_search + end-to-end operator tests; store mutants 0-missed
## Ship
- [ ] tasks all-done; PR merged
