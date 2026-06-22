# S6.4 — Search result match snippets (highlighting) · backlog cleanup

Backlog item from M6. A search result shows the **match context** instead of the generic preview.

## In scope
- Store: the FTS branch of `search_messages` adds `snippet(message_fts, -1, '', '', '…', 10)` and
  uses it as the result's `snippet` (auto-picks the matching column; plain text, since the list
  renders without markup). The stored preview is kept when the snippet is empty.

## Out of scope
- Bold/inline highlight markers (Slint's list Text is plain); snippets for the filter-only branch
  (no MATCH to anchor a snippet).

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. A body-match result's snippet contains the matched term (context), not the stored preview —
   tested; store mutants 0-missed.

## Deliverables
- `snippet()` in the search FTS query + snippet override; test.
