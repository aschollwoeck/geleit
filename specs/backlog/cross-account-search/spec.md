# Backlog — Cross-account search (SEARCH-5)

Search across all accounts, not just the one in view.

## In scope
- Store: `search_all_accounts(query, limit) -> Vec<(MessageHeader, account_id)>` — same FTS / operators
  / snippet as `search_messages` but unscoped, tagging each hit with its account.
- App: an opt-in **"All accounts"** toggle in the search bar (shown only with >1 account). Results
  carry `MessageItem.account`; opening a hit from another account switches `current-account` to it
  (so the rail + actions target the right account) then opens it.

## Out of scope
- A persistent unified inbox (MULTI-3); keeping the result list visible after opening a foreign hit
  (we navigate into that account instead).

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. `search_all_accounts` spans accounts with ids, honors operators/filters, and surfaces match
   snippets — tested; store mutants 0-missed. The open-and-switch UX is the maintainer's eyeball.

## Deliverables
- `search_all_accounts` + test; `MessageItem.account`; "All accounts" toggle + handler; cross-account
  switch on open.
