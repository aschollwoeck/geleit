# S3.4 — Conversation threading · Plan (the HOW)

Implements `spec.md`.

## geleit-engine::thread (pure, mutation- + property-tested)
- `group(items: &[ThreadItem]) -> Vec<Vec<usize>>` where `ThreadItem { message_id: Option<&str>,
  in_reply_to: Option<&str> }`. Union-find: index by `message_id`; for each item whose
  `in_reply_to` matches a known `message_id` in the set, union the two; return components (groups of
  input indices). Items without a usable `message_id` are singletons.
- Tests: parent+reply same group; transitive A←B←C; unlinked → singletons; missing parent →
  singleton; property: partition (disjoint + covers all indices), a reply shares its parent's group.

## geleit-engine::imap
- `fetch_to_new_message`: set `in_reply_to` from `envelope.in_reply_to` (via `decode_header`).

## geleit-store
- Migration **#5**: `ALTER TABLE message ADD COLUMN in_reply_to TEXT`.
- `NewMessage` gains `in_reply_to`; `upsert_message` writes it. `MessageHeader` gains `message_id`
  + `in_reply_to`; `messages_in_folder` selects them. (Existing tests updated for the new fields.)

## geleit-app
- `MessageItem` gains `thread_count: int`. `load_messages`: build `ThreadItem`s from the headers,
  call `thread::group`, and set each row's `thread_count` = its group size. Row shows a small count
  badge when `> 1`. List order unchanged (newest-first, flat).

## Verify
gates; `thread::group` unit+property tests; store round-trip; live (synced reply's `in_reply_to`
stored); app launches; `cargo mutants`.
