# S2.4 — Progressive backfill · Plan (the HOW)

Implements `spec.md`. Reuses S2.3's `reconcile` + store methods.

## geleit-engine::imap (network — mutants-excluded)
- Extract two async helpers (used by incremental **and** backfill):
  - `fetch_envelopes_for(session, store, account_id, folder_id, uid_set: &str)` — `uid_fetch (UID
    ENVELOPE FLAGS INTERNALDATE)` → `upsert_message` (skip UID-less).
  - `fetch_bodies_for(session, store, account_id, folder_id, uid_set: &str)` — `uid_fetch (UID
    BODY.PEEK[])` → `mime::parse_body` → `store_body` (matched by UID).
  Refactor `sync_folder_incremental` to call them.
- `backfill_folder(config, secrets, store, account_id, folder, batch_size, on_batch: &mut dyn
  FnMut(usize)) -> Result<usize, ImapError>`:
  select → `uid_search ALL` → `missing = reconcile(local, server).new` → sort **descending**
  (newest UID first) → for each `batch_size` chunk: `fetch_envelopes_for` + `fetch_bodies_for`;
  `total += chunk.len()`; `on_batch(total)`. Returns total fetched (0 if already complete).

## geleit-app
- `refresh::run_backfill(db_path, secrets, folder, batch_size, on_batch)`: read settings from the
  store (like `run_refresh`) → `backfill_folder`.
- `main.rs` refresh worker: after `run_refresh` Ok → post reload (recent mail shows). Then
  `run_backfill` with `on_batch = |n|` posting `set_status("Catching up… {n}")` + reload via
  `invoke_from_event_loop`. On finish → post `refreshing=false`, `status=""`. (Keep `refreshing=true`
  across the whole thing so the in-flight guard prevents a second run.)

## Tests
- Live (`#[ignore]`, dangerous-tls): append N messages; `sync_folder_incremental(limit=2)` →
  recent 2 local; `backfill_folder(batch=2)` → all N present with bodies; second `backfill_folder`
  → returns 0, `on_batch` not advanced (idempotent). Assert `on_batch` totals are monotonic.

## Verify
gates; live against Dovecot; `cargo mutants` (imap.rs excluded; reconcile/store covered).
