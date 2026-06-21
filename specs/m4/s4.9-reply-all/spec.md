# S4.9 — Reply all (SEND-2 completion) · Spec (the WHAT)

Slice of **M4 (Send)**. Type: store + engine + UI. Completes reply with **Reply all** — replying to
the original sender *and* all other recipients. Deferred from S4.5 because the original To/Cc weren't
stored; this slice captures them and adds the logic + UI.

Status: **draft.**

## In scope
- Store: migration #8 (`to_addrs`/`cc_addrs` on `message`, comma-joined bare addresses); captured on
  envelope sync; read back by `header_by_id`.
- Engine: `message::reply_all(orig, my_addrs, …)` — To = sender + original To (minus me); Cc =
  original Cc (minus me + anyone already in To); de-duplicated case-insensitively. `Original` gains
  `to`/`cc`.
- App: a **Reply all** link in the reading pane (pre-fills compose, including Cc).

## Out of scope
- SPECIAL-USE-based "me" detection beyond the account address; identities/aliases (single address).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings` (incl. `--features dangerous-tls`)/`fmt`/`cargo deny check` green.
2. `to_addrs`/`cc_addrs` round-trip via `header_by_id` (tested).
3. `reply_all` includes other recipients, excludes the account's own address, and de-dups Cc against
   To + itself (tested, incl. overlap/duplicate cases).
4. App Reply-all pre-fills To + Cc (maintainer eyeballs).
5. `cargo mutants` — store + `message` 0-missed.

## Deliverables
- Migration + recipient capture/read + test; `reply_all` + tests; Reply-all link + handler.
