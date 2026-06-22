# S4.14 — Cc address autocomplete (SEND-9 completion) · backlog cleanup

Backlog item: To autocompletes (S4.12) but Cc didn't. Mirror it for Cc.

## In scope
- App: `cc-edited`/`pick-cc-suggestion` + a `c-cc-suggestions` strip under the Cc field, reusing the
  already-tested `store::suggest_addresses`. A shared `address_suggestions` helper backs both To + Cc.
- Fix: both To and Cc now suggest from the **current account** (`ui.get_current_account()`), not
  "the first account" — correct under multi-account (S7.4).
- Cc suggestions are cleared alongside To when a compose/reply/draft opens.

## Out of scope
- Contact groups / a real address book; ranking by frequency or recency.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. Typing in Cc suggests addresses from the current account's history; clicking fills (maintainer
   eyeballs; the underlying `suggest_addresses` is mutation-tested).

## Deliverables
- `c-cc-suggestions` + cc-edited/pick-cc-suggestion; `address_suggestions` helper; current-account fix.
