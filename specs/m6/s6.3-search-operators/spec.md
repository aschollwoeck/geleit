# S6.3 — Search operators (SEARCH-4) · Spec · backlog cleanup

Backlog item from M6. Field-scoped + filter operators in the search box.

## In scope
- Store: `parse_search(input) -> ParsedSearch { match_query, require_attachment }` replacing the old
  `fts_query`. Operators: `from:TEXT` → `sender:` column, `subject:TEXT` → `subject:` column,
  `has:attachment(s)` → a `has_attachments = 1` SQL filter. Bare words search all columns. Terms are
  quoted (no FTS injection); the last full-text term keeps the `*` prefix (type-ahead).
- `search_messages` handles three shapes: full-text (± attachment filter), filter-only
  (`has:attachment` alone → newest with attachments), and empty (no rows). `header_from_row` helper
  dedupes the row mapping.
- App: search-box placeholder hints the operators.

## Out of scope
- `to:`/`cc:`/date-range operators; boolean OR/grouping; cross-account search (SEARCH-5).

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. `parse_search` quoting/operators/filter parsing tested; operators verified end-to-end against FTS
   (column scoping, attachment filter, filter-only, empty); store mutants 0-missed.

## Deliverables
- `parse_search`/`ParsedSearch`/`fts_phrase`/`strip_prefix_ci`/`header_from_row`; `search_messages`
  rewrite; placeholder hint; tests.
