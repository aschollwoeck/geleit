//! The background sync scheduler — how mail arrives without the user pressing anything.
//!
//! Lives in the **host**, not the frontend: a webview throttles or freezes timers in a hidden or
//! occluded window — which is exactly the case this feature exists for — and the frontend only knows
//! the account you're *looking at*, while the host can just ask the store for all of them.
//!
//! Every sync goes through [`crate::ipc::sync_folder_once`], so it takes the folder's sync lock and
//! can never run over a Refresh the user pressed (or a previous tick that overran).
//!
//! The **decisions** (how long to wait, what counts as a failure, which accounts to try) are pure and
//! live in [`crate::schedule`], where they are unit-tested. This file is the glue that acts on them.

use crate::ipc::AppState;
use crate::schedule::{backoff, should_try, sweep_verdict};
use std::collections::HashMap;
use std::time::Duration;
use tauri::{Emitter, Manager};

/// Wait before the *first* sweep, so we don't fight the app's own boot: the UI is already refreshing
/// the account it opens on, and a cold start is the one moment latency is visible (P3).
const FIRST_SWEEP_DELAY: Duration = Duration::from_secs(30);

/// Dev-only: `GELEIT_SYNC_SECS=<n>` replaces the interval **and the backoff**, because a 5-minute poll
/// can't be watched by hand. Debug builds only — in release the env var is never read.
#[cfg(debug_assertions)]
fn interval_override() -> Option<Duration> {
    std::env::var("GELEIT_SYNC_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .map(Duration::from_secs)
}
#[cfg(not(debug_assertions))]
fn interval_override() -> Option<Duration> {
    None
}

/// Start the scheduler. Runs for the life of the app on Tauri's async runtime.
pub(crate) fn spawn(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let fast = interval_override();
        let wake = app.state::<AppState>().wake_sync();

        let mut tick: u64 = 0;
        let mut failures: u32 = 0;
        let mut account_failures: HashMap<i64, u32> = HashMap::new();

        sleep_or_wake(fast.unwrap_or(FIRST_SWEEP_DELAY), &wake).await;
        loop {
            match sweep(&app, tick, &mut account_failures).await {
                Ok(new_mail) => {
                    failures = 0;
                    if new_mail > 0 {
                        // Tell the UI to re-list. It goes through the same `request` epoch as every
                        // other re-list, so an arrival can never clobber a search being typed or a
                        // folder just switched to.
                        let _ = app.emit("mail-arrived", new_mail as i64);
                    }
                }
                // A failed sweep is ordinary — the machine sleeps, the wifi drops. Never surface it:
                // an error the user didn't ask for, about a sync they didn't start, is noise.
                Err(()) => failures = failures.saturating_add(1),
            }
            tick = tick.wrapping_add(1);
            sleep_or_wake(fast.unwrap_or_else(|| backoff(failures)), &wake).await;
        }
    });
}

/// Wait for `delay` — **or** until something says the network is probably back.
///
/// This matters more than it looks. `tokio::time::sleep` is monotonic, and a monotonic clock does not
/// advance while the machine is suspended: close the lid mid-wait and the remaining time still has to
/// elapse *awake* once it opens. And a laptop that was offline overnight has backed off to the
/// half-hour cap — so without this, mail could be 30 minutes stale exactly when the user sits down.
///
/// A successful user-pressed Refresh is the strongest signal we have that we're online again, so
/// `ipc::refresh` fires this: the scheduler stops waiting and sweeps at once.
async fn sleep_or_wake(delay: Duration, wake: &tokio::sync::Notify) {
    tokio::select! {
        () = tokio::time::sleep(delay) => {}
        () = wake.notified() => {}
    }
}

/// One pass over the accounts' inboxes. Returns how many messages arrived that are worth announcing.
async fn sweep(
    app: &tauri::AppHandle,
    tick: u64,
    account_failures: &mut HashMap<i64, u32>,
) -> Result<usize, ()> {
    let state = app.state::<AppState>().inner().clone();
    let Ok(accounts) = crate::ipc::account_ids(&state).await else {
        return Err(());
    };

    let (mut news, mut failed, mut tried) = (0usize, 0usize, 0usize);
    for account_id in &accounts {
        let so_far = account_failures.get(account_id).copied().unwrap_or(0);
        // A failing account is tried progressively less often. "Unreachable" and "wrong password"
        // look identical from here but behave very differently — and a client that retries a revoked
        // login every five minutes, unattended, for days, is how a provider decides to lock an
        // account or block an IP.
        if !should_try(tick, so_far) {
            continue;
        }
        tried += 1;
        match crate::ipc::sync_folder_once(&state, *account_id, "INBOX").await {
            Ok(outcome) => {
                account_failures.remove(account_id); // recovered
                news += outcome.worth_announcing().len();
            }
            Err(_) => {
                failed += 1;
                *account_failures.entry(*account_id).or_insert(0) += 1;
            }
        }
    }
    // Judge the sweep on what it actually *tried* — an account we deliberately skipped isn't a
    // failure, and shouldn't drag the whole schedule into backoff.
    sweep_verdict(news, failed, tried)
}
