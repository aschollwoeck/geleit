//! IMAP IDLE (RFC 2177) — noticing new mail in seconds instead of on the 5-minute poll.
//!
//! One task per account holds an IDLE connection to its INBOX. When the server pushes, the task does
//! the smallest possible thing: it **wakes the sync scheduler** (`AppState::wake_sync`), which already
//! syncs, notifies, and updates the badge. So IDLE is a low-latency *trigger* layered on the existing
//! machinery, not a second sync path — and the periodic poll stays as the safety net (for servers
//! without IDLE, for the reconnect gaps, and for folders other than the INBOX).
//!
//! The engine's [`geleit_engine::imap::idle_watch`] owns the connection and the re-IDLE loop;
//! this file is the glue that resolves each account's config, reconnects with backoff when the
//! connection drops, and stops cleanly when an account is removed or its server has no IDLE.

use crate::ipc::AppState;
use std::time::Duration;
use tauri::Manager;

/// Wait this long before the first IDLE attempt, so it doesn't fight the app's own boot sync.
const FIRST_DELAY: Duration = Duration::from_secs(5);
/// Reconnect backoff after a dropped IDLE connection: gentle, capped. IDLE drops are ordinary (a
/// laptop sleeps, wifi blips), and the poll covers the gap, so there's no hurry.
const RECONNECT_MIN: Duration = Duration::from_secs(10);
const RECONNECT_MAX: Duration = Duration::from_secs(5 * 60);

/// Start an IDLE watcher for every account. Runs for the life of the app on Tauri's async runtime.
///
/// An account **added while the app is running** gets its watcher at once via [`watch_new_account`], so
/// it no longer waits for the next launch for instant push — the poll was always its safety net, and now
/// isn't its only fast path.
pub(crate) fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_DELAY).await;
        let state = app.state::<AppState>().inner().clone();
        let Ok(accounts) = crate::ipc::account_ids(&state).await else {
            return;
        };
        for account_id in accounts {
            start_watch(app.clone(), account_id);
        }
    });
}

/// Start watching an account that was **just added**, so it gets instant push without a restart. A
/// no-op if it's already watched (re-configuring an existing account keeps its one watcher).
pub(crate) fn watch_new_account(app: &tauri::AppHandle, account_id: i64) {
    start_watch(app.clone(), account_id);
}

/// Spawn the watcher for `account_id` — but only if one isn't already running, so an account is never
/// watched by two tasks (double connections, double wakes).
fn start_watch(app: tauri::AppHandle, account_id: i64) {
    let Some(cancel) = app.state::<AppState>().inner().claim_idle_watch(account_id) else {
        return; // already watched
    };
    tauri::async_runtime::spawn(async move { watch_account(app, account_id, cancel).await });
}

/// Keep an IDLE connection to one account's INBOX alive, reconnecting with backoff when it drops, until
/// `cancel` fires (the account was removed). `cancel` is also the slot token, so freeing the slot on exit
/// can't evict a replacement watcher that reused this id.
async fn watch_account(
    app: tauri::AppHandle,
    account_id: i64,
    cancel: std::sync::Arc<tokio::sync::Notify>,
) {
    let state = app.state::<AppState>().inner().clone();
    // However this task leaves (cancelled, account gone, no server IDLE), free *its own* slot so the
    // account can be watched again if it's re-added.
    struct Release {
        state: AppState,
        account_id: i64,
        cancel: std::sync::Arc<tokio::sync::Notify>,
    }
    impl Drop for Release {
        fn drop(&mut self) {
            self.state.release_idle_watch(self.account_id, &self.cancel);
        }
    }
    let _release = Release {
        state: state.clone(),
        account_id,
        cancel: cancel.clone(),
    };
    let mut backoff = RECONNECT_MIN;
    loop {
        // Resolve the account's config on a worker (it opens the encrypted store). Gone → stop the
        // task; the account was removed.
        let (db, secrets) = (state.db_path.clone(), state.secrets.clone());
        let config = tauri::async_runtime::spawn_blocking(move || {
            geleit_engine::sync_actions::account_imap(&db, &*secrets, account_id)
        })
        .await;
        let Ok(Ok(config)) = config else {
            return; // account removed (or unreadable) — nothing to watch
        };

        // Wake the scheduler on any server push, the same poke a successful Refresh uses — so an IDLE
        // event and a Refresh drive the identical sweep. `notify_one` (not `notify_waiters`) so a push
        // that lands while the scheduler is mid-sweep isn't lost: it stores a permit that triggers an
        // immediate follow-up sweep, rather than waiting out the whole poll interval.
        let wake = state.wake_sync();
        let on_activity = move || wake.notify_one();

        // `idle_watch` only ever returns an error — it loops forever otherwise. Race it against `cancel`
        // so a removed account drops its connection at once, instead of lingering (authenticated) until
        // the next server push or timeout.
        let started = std::time::Instant::now();
        let outcome = tokio::select! {
            () = cancel.notified() => return, // account removed — stop now
            r = geleit_engine::imap::idle_watch(&config, &*state.secrets, "INBOX", &on_activity) => r,
        };
        match outcome {
            // The server has no IDLE — stop; the poll is this account's only path, and that's fine.
            Err(geleit_engine::imap::ImapError::IdleUnsupported) => return,
            // A dropped connection (a laptop sleeps, wifi blips): wait, then reconnect. Ordinary and
            // never surfaced — the user didn't ask, and the poll covers the gap.
            Err(_) => {
                // A connection that lasted a good while was healthy and merely dropped — reconnect
                // promptly. One that failed fast is genuinely unreachable, so keep backing off (a
                // client hammering a server it can't reach is how a provider decides to block it).
                if started.elapsed() >= RECONNECT_MAX {
                    backoff = RECONNECT_MIN;
                }
                // The reconnect wait also races cancel — a removal during backoff shouldn't wait it out.
                tokio::select! {
                    () = cancel.notified() => return,
                    () = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(RECONNECT_MAX);
            }
        }
    }
}
