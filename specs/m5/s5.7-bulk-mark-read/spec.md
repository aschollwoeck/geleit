# S5.7 — Bulk mark-as-read (ORG-7 extension) · backlog cleanup

Backlog item: the bulk-action bar had Archive/Delete/Star; add **Mark read** for fast inbox cleanup.

## In scope
- App: a "Mark read" entry in the bulk bar + `bulk-mark-read` handler — `store.set_seen(id, true)` for
  each selected message, flip the row to read in place, clear the selection.

## Out of scope
- Server write-back of `\Seen` (read state is local today, like single mark-unread — SYNC-5 follow-up);
  bulk mark-*unread*.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. Selecting messages + Mark read clears their unread state + the selection (maintainer eyeballs;
   `set_seen` is already covered).

## Deliverables
- `bulk-mark-read` callback + bar button + handler.
