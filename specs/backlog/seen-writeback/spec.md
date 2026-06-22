# Backlog — Server read-state write-back (\Seen, SYNC-5)

Read state was local-only (star/\Flagged already wrote back; read didn't). Now marking read/unread
syncs to the server.

## In scope
- Engine: factor a shared `store_flag(folder, uid, add, flag)` (`UID STORE +/-FLAGS`); `set_flag`
  (\Flagged) + new `set_seen` (\Seen) on top.
- Store: `message_location(id) -> (folder_name, uid)` so write-back uses the message's REAL folder
  (correct even when opened from a cross-folder search result); `None` for local-only/gone.
- Refresh: `run_set_seen`. App: best-effort `\Seen` write-back on open-marks-read, mark-unread, and
  bulk-mark-read (one worker for the batch). Read state stays correct locally if the push fails.

## Out of scope
- Server→local read-state reconciliation beyond first sync; offline queueing of the write-back.

## Acceptance criteria
1. build/test/clippy -D warnings (+dangerous-tls)/fmt/`cargo deny check` green.
2. `message_location` tested (folder+uid; None for local-only/absent); store mutants 0-missed.
   Engine `set_seen`/`store_flag` are live-tested glue; the sync is the maintainer's eyeball.

## Deliverables
- `store_flag`/`set_seen`; `message_location` + test; `run_set_seen`; 3 app write-back points.
