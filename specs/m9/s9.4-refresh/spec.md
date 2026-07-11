# S9.4 — Refresh & sync, with calm progressive feedback

**Milestone:** M9. **Constitution:** P1 (local-first — the UI never waits on the network), P3 (calm
and fast; feedback is the fallback, instantaneity the goal), P6 (integrity — sync never loses/dupes).

## What it delivers

The **Refresh** action, and the safety net S9.3's optimistic actions rely on: talk to the server,
pull new mail newest-first, and backfill the rest quietly in the background — with a calm progress
line, never a blocked UI.

| | Story | Acceptance |
|---|---|---|
| **S9.4-1** | I press Refresh and get new mail. | Refresh syncs the folder list + the current folder's recent envelopes; the list updates when done. |
| **S9.4-2** | The app stays responsive while it syncs. | Sync runs on a worker; the UI never blocks (P1). A quiet "Refreshing…" state shows on the button. |
| **S9.4-3** | I can see it catching up. | After the first sync, older mail backfills in the background with a calm progress line ("Catching up… N"), distinct from an error. |
| **S9.4-4** | If it can't reach the server, it says so calmly. | A short, PII-free message; existing mail stays put. |
| **S9.4-5** | My optimistic actions reconcile. | A refresh restores truth after any action whose server write-back failed (the S9.3 safety net). |

## How

- **Reuse:** `imap::sync_folders` / `sync_folder_incremental` / `backfill_folder` already do the work
  (M1/M2). The wrappers `run_refresh` / `run_backfill` (and `run_remove_account`, needed by S9.6)
  move from the Slint `refresh.rs` into `geleit_engine::sync_actions`, re-exported so the Slint app
  is unchanged — same pattern as S9.3's action write-backs.
- **Progress streaming:** a `refresh` command spawns a worker that runs the sync, then the backfill,
  **emitting Tauri events** (`sync-progress`) as batches land. The frontend listens and shows the
  progress line. This is the one new mechanism (the shell↔frontend event channel).
- **After completion**, the frontend re-lists the current folder from the store — so new mail appears
  and any failed-write-back divergence heals.

## Out of scope

Compose (S9.5); search/settings/accounts (S9.6); background auto-sync on a timer (a follow-up —
refresh stays manual, as it is today); server→local flag read-back beyond what M6 already did.
