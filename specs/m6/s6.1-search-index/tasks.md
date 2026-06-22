# S6.1 — Search index (FTS5) · Tasks
## Build
- [x] ADR-0010 (FTS5 over tantivy — encryption at rest)
- [x] migration #10: message_fts FTS5 + AFTER DELETE trigger
- [x] fts_query pure helper; index_message; reindex_all; search_messages; open backfill
- [x] wire index_message into upsert_message + store_body
## Verify
- [x] AC1 build/test/clippy(+dangerous-tls)/fmt/deny green
- [x] AC2 index/search/prefix/account-scope/delete-cascade/backfill tested
- [x] AC3 fts_query tested
- [x] AC4 mutants store 0-missed
## Ship
- [ ] tasks all-done; PR merged
