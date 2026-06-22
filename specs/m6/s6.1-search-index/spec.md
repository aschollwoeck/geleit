# S6.1 — Search index (SQLite FTS5) (SEARCH-1/2/3 backend) · Spec

First slice of **M6 (Search)**. Build the full-text index + query primitive. Per **ADR-0010**, search
uses SQLite **FTS5** inside the SQLCipher DB (encrypted at rest) — not an external engine whose files
would be plaintext on disk. Covers the roadmap's S6.1 (index) + S6.2 (incremental) backend; the UI is
S6.2 here.

## In scope
- Migration #10: `message_fts` FTS5(subject, sender, body), rowid = message id, + an `AFTER DELETE ON
  message` trigger (fires on FK cascades too).
- `index_message(id)` — (re)build a row from subject/sender/plain body; wired into `upsert_message`
  (envelope) and `store_body` (body). `reindex_all` + a one-time open **backfill** for pre-#10 data.
- `fts_query(input)` — free text → safe `MATCH` (quoted phrases + trailing `*` prefix); `None` if empty.
- `search_messages(account, query, limit)` — FTS hits joined to message rows, `ORDER BY rank`.

## Out of scope
- Search UI (S6.2). Operators (from:/has:attachment/date — SEARCH-4) + cross-account (SEARCH-5) — later.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. FTS5 confirmed available in the bundled SQLCipher; subject/sender/body searchable; prefix matches;
   account-scoped; delete (incl. cascade) unindexes; backfill rebuilds on open — all tested.
3. `fts_query` quoting/prefix/empty-guard tested.
4. `cargo mutants` — store 0-missed.

## Deliverables
- ADR-0010; migration #10 + trigger; `fts_query`, `index_message`, `reindex_all`, `search_messages`,
  open backfill; tests.
