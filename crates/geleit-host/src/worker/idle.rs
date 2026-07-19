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

use crate::AppState;
use std::time::Duration;

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
pub async fn run(state: AppState) {
    tokio::time::sleep(FIRST_DELAY).await;
    let Ok(accounts) = crate::commands::account_ids(&state).await else {
        return;
    };
    for account_id in accounts {
        // We're inside a spawned task here (a live tokio runtime), so spawning each watcher directly
        // is fine — `watch_new_account` hands back the future and this loop drives it onto the runtime.
        if let Some(watcher) = watch_new_account(&state, account_id) {
            tokio::spawn(watcher);
        }
    }
}

/// Prepare an IDLE watcher for an account that was **just added**, so it gets instant push without a
/// restart — returning the watcher future for the caller to spawn, or `None` if it's already watched
/// (which is how an account is never watched by two tasks: double connections, double wakes).
///
/// Deliberately **spawn-agnostic**: it does not touch any runtime itself, so a host can call it from a
/// command handler and spawn the result on *its own* executor (`tauri::async_runtime::spawn` on the
/// desktop, `tokio::spawn` on the web host) without assuming an ambient tokio runtime is present.
#[must_use]
pub fn watch_new_account(
    state: &AppState,
    account_id: i64,
) -> Option<impl std::future::Future<Output = ()> + Send + 'static> {
    let cancel = state.claim_idle_watch(account_id)?; // already watched → nothing to spawn
    let state = state.clone();
    Some(async move { watch_account(state, account_id, cancel).await })
}

/// Keep an IDLE connection to one account's INBOX alive, reconnecting with backoff when it drops, until
/// `cancel` fires (the account was removed). `cancel` is also the slot token, so freeing the slot on exit
/// can't evict a replacement watcher that reused this id.
async fn watch_account(
    state: AppState,
    account_id: i64,
    cancel: std::sync::Arc<tokio::sync::Notify>,
) {
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
        let config = tokio::task::spawn_blocking(move || {
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
