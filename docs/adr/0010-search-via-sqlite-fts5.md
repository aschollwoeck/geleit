# ADR-0010 — Full-text search via SQLite FTS5 (not a separate engine)

- **Status:** Accepted
- **Date:** 2026-06-22
- **Context:** M6 (Search). SEARCH-1/2/3 + OFF-2 — fast, offline search over sender/subject/body.

## Decision
Implement search with **SQLite's built-in FTS5** virtual table, living **inside the existing
SQLCipher database**, rather than an external index engine (the roadmap's provisional `tantivy`).

## Why
1. **Encryption at rest (SEC-1) is preserved for free.** Our DB is SQLCipher-encrypted (ADR-0008).
   An FTS5 table is just more SQLite content, so the index — subjects, sender names, **full body
   text** — is encrypted on disk like everything else. A separate engine (tantivy/Lucene-style)
   writes its own segment files in **plaintext**, which would leak exactly the content we promise to
   keep private. This alone is decisive for a privacy-first client.
2. **Offline by construction (SEARCH-2/OFF-2).** The index is the local DB; there is nothing to
   reach for and no second store to keep consistent or back up.
3. **Simplicity & consistency.** No new heavy dependency, no second on-disk store, no cross-store
   transactionality problem: indexing happens in the same connection/transactions as the writes
   that produce it.
4. **Fast enough (SEARCH-3).** FTS5 with bm25 ranking is near-instant at a single user's mail
   volume; performance is not the binding constraint, privacy is.

## How
- Migration #10: `message_fts` FTS5(subject, sender, body), rowid = message id; an `AFTER DELETE ON
  message` trigger removes the row (also fires on FK cascades from folder/account deletes).
- `index_message(id)` (re)builds a row from subject/sender/plain-body; called after `upsert_message`
  (envelope) and again after `store_body` (body). `reindex_all` + a one-time open backfill cover
  messages that predate the migration.
- `fts_query` turns free user input into a safe `MATCH` string (each token a quoted phrase; trailing
  `*` for prefix/type-ahead) so punctuation can't inject FTS operators.
- `search_messages(account, query, limit)` joins FTS hits to message rows, `ORDER BY rank`.

## Consequences / follow-ups
- Search operators (`from:`, `has:attachment`, date ranges — SEARCH-4) and cross-account search
  (SEARCH-5) are deferred; they layer cleanly on FTS5 (separate columns / multiple `MATCH`).
- If volume ever outgrows FTS5 (not expected for personal mail), revisit — but any replacement must
  keep the index encrypted at rest.
