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
/// Accounts added later fall back to the poll until the next launch — a named limitation, not a bug:
/// the 5-minute scheduler still delivers their mail.
pub(crate) fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_DELAY).await;
        let state = app.state::<AppState>().inner().clone();
        let Ok(accounts) = crate::ipc::account_ids(&state).await else {
            return;
        };
        for account_id in accounts {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { watch_account(app, account_id).await });
        }
    });
}

/// Keep an IDLE connection to one account's INBOX alive, reconnecting with backoff when it drops.
async fn watch_account(app: tauri::AppHandle, account_id: i64) {
    let state = app.state::<AppState>().inner().clone();
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

        // `idle_watch` only ever returns an error — it loops forever otherwise.
        let started = std::time::Instant::now();
        match geleit_engine::imap::idle_watch(&config, &*state.secrets, "INBOX", &on_activity).await
        {
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
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(RECONNECT_MAX);
            }
        }
    }
}
