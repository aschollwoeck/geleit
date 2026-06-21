# S4.12 — Address autocomplete (SEND-9) · Spec

Slice of **M4**. Suggest recipient addresses from mail history as you type the **To** field — no
separate address book.

## In scope
- Store: `suggest_addresses(account_id, prefix, limit)` — distinct senders (`from_addr`) seen in this
  account, case-insensitive prefix, alphabetical, LIKE-wildcards escaped. Tested.
- App: `Field` gains an `edited` callback; typing in To queries suggestions and shows a small list
  under it; clicking one completes the current token (leaving ", " for the next). `last_token` /
  `complete_last_token` pure helpers (tested).

## Out of scope
- Cc autocomplete; ranking by frequency/recency; suggesting addresses you've *written* to (only
  received senders for now). Fuzzy/substring match (prefix only).

## Acceptance criteria
1. build/test/clippy -D warnings/fmt/`cargo deny check` green.
2. `suggest_addresses` prefix/distinct/sorted/escaped — tested; `last_token`/`complete_last_token` —
   tested.
3. App: typing in To shows matching addresses; clicking fills the token; suggestions cleared on
   open/reply/forward/resume (maintainer eyeballs).
4. `cargo mutants` — store + viewmodel 0-missed.
