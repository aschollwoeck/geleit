# S3.4 — Conversation threading · Spec (the WHAT)

Slice of **M3**. Type: engine + store + UI. Delivers **READ-5** (detect conversations): messages
that reply to one another are recognised as a thread, and the list shows how many messages are in
each conversation. Full thread *navigation* (open a conversation, see all its messages together) is
a follow-up; this slice lands the threading **data + algorithm + indicator** without reordering the
list (so no message becomes unreachable).

Status: **draft.**

## Purpose
Recognise conversations from the `Message-ID` / `In-Reply-To` headers and show a small count on
messages that belong to a multi-message thread — the foundation for richer thread views later.

## In scope
- Fetch + store `In-Reply-To` (`Message-ID` is already stored).
- `geleit-engine::thread::group(items) -> Vec<Vec<usize>>`: cluster messages linked by
  `in_reply_to ↔ message_id` (connected components) — pure, unit- + property-tested.
- `geleit-app`: the message list shows a conversation count (e.g. a "3" badge) on messages whose
  thread has more than one member. The list stays newest-first and flat (every message reachable).

## Out of scope
- Collapsing the list into one row per thread / opening a whole conversation (follow-up). Threading
  across folders or by subject (we use the standards-based reference links only). HTML rendering.

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. `thread::group` correct: a reply lands in its parent's group; transitive chains group; unlinked
   messages are singletons; robust to a missing parent (not in our set) — unit + **property** tested
   (partition: disjoint, covers all; a reply shares its parent's group).
3. store: `in_reply_to` round-trips; `messages_in_folder` returns `message_id` + `in_reply_to`.
4. **Live (`--features dangerous-tls`):** a synced reply has its `in_reply_to` stored.
5. UI: messages in a multi-message conversation show a count; the list stays flat (no message hidden).
6. `cargo mutants` — `thread::group` + store covered; imap.rs/main.rs excluded; 0 missed.

## Deliverables
- `in_reply_to` fetch+store; `engine::thread` + tests; conversation-count UI; live test; manual touch.
