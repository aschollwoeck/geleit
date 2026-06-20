# S2.3 — Incremental sync · Spec (the WHAT)

Slice of **M2**. Type: engine + store. Makes sync **robust** (SYNC-1†): a refresh now reflects
**new** *and* **deleted** messages, and survives a server-side UID reset — instead of the M1 naive
window that only ever added recent envelopes and never noticed deletions.

Status: **draft.**

## Purpose
On refresh, reconcile local state against the server: fetch envelopes+bodies for genuinely **new**
UIDs (not the whole window again) and **remove** messages deleted on the server, keyed safely by
UID with **UIDVALIDITY** handling.

## In scope
- Pure `reconcile(local_uids, server_uids) -> {new, deleted}` (engine, mutation-tested).
- `geleit-store`: per-folder `uid_validity` (migration #3); `uids_in_folder`,
  `delete_messages_by_uid`, `clear_folder`, `set_folder_uidvalidity`/`folder_uidvalidity`.
- `geleit-engine::imap::sync_folder_incremental`: select → UIDVALIDITY check (reset the folder if it
  changed) → `uid_search ALL` → reconcile → delete gone UIDs; fetch envelopes+bodies for new UIDs
  (capped to the most-recent N — older backfill is S2.4). Wired into `run_refresh`/`run_setup`.

## Out of scope
- **Flag-change sync** (server→local read/flag updates): deferred to **M6** with server write-back,
  where the local-vs-server conflict is resolved (syncing flags now would clobber local read-state,
  regressing READ-7). CONDSTORE/QRESYNC MODSEQ optimization (follow-up). Full-mailbox backfill (S2.4).

## Acceptance criteria (measurable)
1. build/test/`clippy -D warnings`/`fmt`/`cargo deny check` green.
2. `reconcile` correct (new = server−local, deleted = local−server; empty/disjoint/equal cases) — tested.
3. store: `uids_in_folder`/`delete_messages_by_uid` (cascades bodies)/`clear_folder`/uidvalidity
   round-trip — tested; migration #3 applies on an existing db.
4. **Live (`--features dangerous-tls`):** append a message → sync → it appears; delete a message on
   the server → sync → it's gone locally; (re-sync is idempotent — no dupes/loss).
5. P1/P2 unchanged (sync off the UI thread; no PII in errors).
6. `cargo mutants` — `reconcile` + store methods covered; `imap.rs` excluded; 0 missed.

## Deliverables
- `reconcile`; store uidvalidity + uid methods; `sync_folder_incremental` wired into refresh;
  live test (new + deleted). *(No new ADR.)*
