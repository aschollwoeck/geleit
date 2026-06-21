# S4.5 — Reply & Forward (SEND-2/3) · Spec (the WHAT)

Slice of **M4 (Send)**. Type: engine + UI. Adds **Reply** (to sender) and **Forward** on the open
message, pre-filling the compose window with correct quoting, `Re:`/`Fwd:` subjects, and conversation
threading headers.

Status: **draft.**

## In scope
- `engine::message`: `Draft` gains `in_reply_to` + `references`; `build()` emits the `In-Reply-To` /
  `References` headers. `reply(orig, …)` / `forward(orig, …)` + an `Original` input; pure helpers for
  subject-prefixing (no `Re:`/`Fwd:` doubling), attribution-line quoting, and the references chain.
- Store: `header_by_id` (fetch one message's header for reply/forward).
- App: **Reply** / **Forward** links in the reading pane → pre-fill the compose overlay; threading
  headers carried through `run_send`.

## Out of scope
- **Reply-all** — needs the original To/Cc, which aren't stored yet (a future migration). Deferred.
- Attachments on forward (S4.4 roadmap). Drafts (S4.5). HTML-quote (text-quote only here).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. `reply` targets the sender, prefixes `Re:` (no doubling), quotes with an attribution line, and sets
   `in_reply_to` + `references`; `forward` prefixes `Fwd:`, includes the original, leaves recipients
   empty; `build()` emits the threading headers — all unit-tested. `header_by_id` round-trips (tested).
3. App: Reply/Forward open compose pre-filled; a sent reply carries `In-Reply-To`/`References`
   (maintainer eyeballs the live send).
4. `cargo mutants` — `message` + store 0-missed.

## Deliverables
- `reply`/`forward`/threading in `engine::message` + tests; `store::header_by_id` + test; UI links +
  threading carry; manual note.
