# S6.2 — Search UI (SEARCH-1/2/3, OFF-2) · Spec

Final M6 slice. A search box over the message list; results from the local FTS index (S6.1).

## In scope
- App: a search Field in the list-pane header; typing queries `store.search_messages` on each
  keystroke (FTS5 is sub-ms → run synchronously on the UI thread, instant — no worker). Results
  replace the list (relevance order); a "N found · Clear" affordance + count. Empty query or Clear or
  switching folders returns to the folder. Opening a result works (real message ids).

## Out of scope
- Search operators (SEARCH-4) + cross-account (SEARCH-5); highlight/snippets of the match.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. Typing searches sender/subject/body offline + instant; results open; Clear / folder-switch exits
   search (maintainer eyeballs; built on the mutation-tested `search_messages` + `message_vm`).

## Deliverables
- search-query/searching/search-count props + search-edited/clear-search callbacks; search bar UI;
  `search_result_items`; handlers; folder-switch clears search.
