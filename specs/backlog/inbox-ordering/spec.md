# Backlog — Inbox-first folder ordering

The folder rail sorted alphabetically (`ORDER BY name`), so "Archive" appeared above "Inbox" and the
app — which opens the first folder — opened to Archive, not the Inbox. Found while taking screenshots.

## In scope
- Store: `folders_for_account` now orders by a conventional rank — **Inbox**, Drafts, Sent, Archive,
  Junk, Trash, then everything else — with same-rank ties broken alphabetically (case-insensitive).
  A pure `folder_rank(name)` does the ranking (matches provider variants like "Deleted Items",
  "Sent Mail" loosely). This also fixes the default view: the app opens to the Inbox.

## Out of scope
- User-customisable folder order; nested-folder hierarchy display.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. Ordering tested across all ranks + variants + alphabetical tail; store mutants 0-missed.
3. Screenshots refreshed: the rail reads INBOX, Sent, Archive, Junk, Trash and opens to INBOX.

## Deliverables
- `folder_rank` + reordered `folders_for_account` + test; refreshed `inbox-{light,dark}.png`.
