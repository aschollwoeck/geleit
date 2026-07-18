//! Background full-mailbox backfill (SYNC-3) — progressively pulling **every** account's older mail
//! down to the device, so search and offline reading are complete, not recent-only.
//!
//! The scheduler ([`crate::scheduler`]) keeps each account's INBOX *recent* mail fresh; this worker does
//! the slower, low-priority job of catching up the rest — all of every folder, one folder at a time — so
//! a secondary account you rarely open still becomes fully searchable and available offline. `run_backfill`
//! is resumable (it fetches server-minus-local each time), so a finished folder is a cheap near-no-op and
//! a partial one picks up where it left off; the worker just keeps sweeping until everything is local, then
//! re-scans hourly to catch new accounts, new folders, and any gaps.
//!
//! It shares the per-`(account, folder)` backfill single-flight with a user-pressed Refresh
//! ([`AppState::try_begin_backfill`]), so the two never download the same folder at once. That guard keys
//! on the stored folder name (as Refresh does); it does not down-case, so it relies on both sides passing
//! the canonical name — a duplicate download, not a correctness bug, if they ever diverge (upserts are
//! idempotent).
//!
//! Deliberately *not* serialized with the scheduler's recent-sync (no shared folder lock): a background
//! catch-up must never make foreground mail wait, so it stays out of the way (small batches, a pause
//! between them) and leans on WAL + idempotent UID upserts, healing any transient overlap on the next
//! sync. Folders and accounts are walked sequentially, so one enormous folder delays *other accounts'
//! backfill* (never their recent mail, which the scheduler keeps fresh regardless) — acceptable for a
//! background job; interleaving accounts is a possible refinement.

use crate::ipc::AppState;
use std::time::Duration;
use tauri::Manager;

/// Wait before the first pass, so it doesn't fight the app's boot sync (the scheduler's first sweep, at
/// 30s, is what populates each account's folder list this pass reads).
const FIRST_DELAY: Duration = Duration::from_secs(60);
/// A breath between folders — the backfill is a background courtesy, never a hammer on the server.
const BETWEEN_FOLDERS: Duration = Duration::from_secs(3);
/// Re-scan this often once caught up: cheap (a finished folder is one `UID SEARCH` round-trip), and it's
/// how a newly-added account, a new folder, or a sync gap gets picked up without waiting for a restart.
const ROUND_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// The backfill batch size — deliberately **smaller** than Refresh's 200. A background download runs for
/// the whole mailbox, so each write burst competes with foreground sync for the store's single WAL writer
/// lock; small batches keep each burst short. See [`BETWEEN_BATCHES`].
const BATCH: u32 = 50;
/// A pause after each batch, so the writer lock is repeatedly free for the scheduler's recent-sync and a
/// user Refresh — the background catch-up must never make foreground mail feel slow ("latency is a
/// defect"). Runs on a blocking worker thread, so a plain sleep is right.
const BETWEEN_BATCHES: Duration = Duration::from_millis(100);

/// Dev-only: `GELEIT_BACKFILL_SECS=<n>` shortens the between-rounds wait so the worker can be watched by
/// hand. Debug builds only.
#[cfg(debug_assertions)]
fn round_override() -> Option<Duration> {
    std::env::var("GELEIT_BACKFILL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .map(Duration::from_secs)
}
#[cfg(not(debug_assertions))]
fn round_override() -> Option<Duration> {
    None
}

/// Start the backfill worker. Runs for the life of the app on Tauri's async runtime.
pub(crate) fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_DELAY).await;
        loop {
            let state = app.state::<AppState>().inner().clone();
            backfill_round(&state).await;
            tokio::time::sleep(round_override().unwrap_or(ROUND_INTERVAL)).await;
        }
    });
}

/// One pass over every account's every server folder, backfilling each in turn.
async fn backfill_round(state: &AppState) {
    let Ok(accounts) = crate::ipc::account_ids(state).await else {
        return;
    };
    for account_id in accounts {
        for folder in server_folders(state, account_id).await {
            // Skip if a Refresh (or a leftover claim) is already on this folder — it'll finish it.
            if !state.try_begin_backfill(account_id, &folder) {
                continue;
            }
            let (db, secrets) = (state.db_path.clone(), state.secrets.clone());
            let f = folder.clone();
            // Blocking + network → a worker thread. A folder that can't be reached (offline) just returns
            // an error and is left for the next round; nothing is surfaced — the user didn't ask. The
            // per-batch pause keeps the background download from starving foreground sync of the writer.
            // Two jobs per folder: backfill older mail, and reconcile the folder against the server —
            // remove server-deleted messages and pull read/star changes made on another device (SYNC-5).
            // The scheduler only does this for INBOX, so this keeps *every* folder in step in the
            // background too. Both best-effort; a failure just waits for the next round.
            let _ = tauri::async_runtime::spawn_blocking(move || {
                geleit_engine::sync_actions::run_backfill(
                    &db,
                    &*secrets,
                    account_id,
                    &f,
                    BATCH,
                    &mut |_| {
                        std::thread::sleep(BETWEEN_BATCHES);
                    },
                )
                .ok();
                geleit_engine::sync_actions::run_reconcile_folder(&db, &*secrets, account_id, &f)
                    .ok();
            })
            .await;
            state.end_backfill(account_id, &folder);
            tokio::time::sleep(BETWEEN_FOLDERS).await;
        }
    }
}

/// The account's folders that live on the server — the local-only **Saved** folder (imported `.eml`) has
/// nothing to backfill and would fail a `SELECT`, so it's left out.
async fn server_folders(state: &AppState, account_id: i64) -> Vec<String> {
    let (db, secrets) = (state.db_path.clone(), state.secrets.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let Ok(store) = geleit_engine::localstore::open_store(&db, &*secrets) else {
            return Vec::new();
        };
        store
            .folders_for_account(account_id)
            .map(|folders| {
                folders
                    .into_iter()
                    .filter(|f| !f.name.eq_ignore_ascii_case(geleit_store::SAVED_FOLDER))
                    .map(|f| f.name)
                    .collect()
            })
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default()
}
