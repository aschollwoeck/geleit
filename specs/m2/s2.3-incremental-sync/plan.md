# S2.3 — Incremental sync · Plan (the HOW)

Implements `spec.md`.

## geleit-engine::sync (pure, mutation-tested)
- `struct SyncPlan { new: Vec<u32>, deleted: Vec<u32> }`.
- `reconcile(local: &[u32], server: &[u32]) -> SyncPlan`: `new` = in server not local; `deleted` =
  in local not server. (Sets; order not relied upon.)

## geleit-store
- Migration **#3** (append-only): `ALTER TABLE folder ADD COLUMN uid_validity INTEGER`.
- `set_folder_uidvalidity(folder_id, v: i64)`, `folder_uidvalidity(folder_id) -> Option<i64>`.
- `uids_in_folder(folder_id) -> Vec<i64>`.
- `delete_messages_by_uid(folder_id, uids: &[i64])` (bodies cascade via FK; chunked IN-list).
- `clear_folder(folder_id)` (delete all messages — UIDVALIDITY reset).
- Tests: round-trip, delete-by-uid cascades, clear, uidvalidity.

## geleit-engine::imap::sync_folder_incremental (network — mutants-excluded)
`(config, secrets, store, account_id, folder, limit) -> Result<usize, ImapError>`:
1. `select(folder)`; read `uid_validity`. If a stored value exists and differs → `clear_folder`
   (UIDs no longer valid). Persist the current `uid_validity`.
2. `server = uid_search("ALL")`; `local = store.uids_in_folder`.
3. `plan = reconcile(&local, &server)`.
4. `store.delete_messages_by_uid(folder_id, plan.deleted)`.
5. New, capped to the `limit` highest UIDs (recent-first; older backfill = S2.4): `uid_fetch(set,
   "(UID ENVELOPE FLAGS INTERNALDATE)")` → `upsert_message`; then `uid_fetch(set, "(UID BODY.PEEK[])")`
   → `mime::parse_body` → `store_body` (reusing the S1.5/S1.6 helpers).
- `run_setup`/`run_refresh` call `sync_folders` + `sync_folder_incremental` (replacing the naive
  `sync_envelopes`+`sync_bodies` pair). `sync_envelopes`/`sync_bodies` stay (own tests/building blocks).

## Tests
- `reconcile`: equal → empty; new-only; deleted-only; mixed; empty local; empty server.
- store methods (as above).
- Live (`#[ignore]`, dangerous-tls): append → sync → present; delete on server → sync → absent;
  sync twice → stable (idempotent).

## Verify
gates; live against Dovecot; `cargo mutants` (reconcile + store; imap.rs excluded).
