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
use crate::notify::{self, QuietHours, Verdict, COLLAPSE_AT};
use crate::schedule::{backoff, should_try, sweep_verdict};
use std::collections::HashMap;
use std::time::Duration;
use tauri::{Emitter, Manager};

/// How many messages we read to build the notification's *text*. Only the senders' names come from
/// these; the **count** is asked of the store, because a mailbox back from a week away needs a true
/// number far more than it needs every sender read out.
const NOTIFY_SAMPLE: i64 = 10;

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
                Ok(changed) => {
                    failures = 0;
                    // The badge is set every sweep, not only when mail arrives: a sweep is also when
                    // mail read on another device comes back `\Seen`, and the count should fall for
                    // that too.
                    crate::ipc::set_badge(&app, app.state::<AppState>().inner()).await;
                    if changed > 0 {
                        // Tell the UI to re-list — for new mail, or for a flag pulled from another
                        // device. It goes through the same `request` epoch as every other re-list, so
                        // it can never clobber a search being typed or a folder just switched to.
                        let _ = app.emit("mail-arrived", changed as i64);
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

/// One pass over the accounts' inboxes. Returns how many accounts had a change the UI should re-list
/// for — mail that arrived, or a read/star flag pulled from another device (SYNC-5).
async fn sweep(
    app: &tauri::AppHandle,
    tick: u64,
    account_failures: &mut HashMap<i64, u32>,
) -> Result<usize, ()> {
    let state = app.state::<AppState>().inner().clone();
    let Ok(accounts) = crate::ipc::account_ids(&state).await else {
        return Err(());
    };

    let (mut changed, mut failed, mut tried) = (0usize, 0usize, 0usize);
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
        // Drain any read/star changes queued for this account (SYNC-5) before pulling — a change made
        // offline reaches the server here, and going out first means the sync's flag pull sees a
        // server that already agrees rather than a stale one.
        crate::ipc::flush_flags(&state, *account_id).await;
        // Send anything waiting in the outbox (SEND-10): mail composed while offline goes out now that
        // we've reached the server. A message that goes out lands in Sent, so the list is stale too.
        if crate::ipc::flush_outbox(&state, *account_id).await > 0 {
            changed += 1;
        }
        match crate::ipc::sync_folder_once(&state, *account_id, "INBOX").await {
            Ok(outcome) => {
                account_failures.remove(account_id); // recovered
                                                     // A re-list is due if mail arrived OR a flag was pulled from another device — either
                                                     // way the on-screen list is now stale, even when nothing is *announced*.
                if !outcome.arrived.is_empty() || outcome.flag_updates > 0 {
                    changed += 1;
                }
                // Tell the user — from the **store**, not from this sync's diff. A message the backfill
                // swept up, or one that arrived while notifications were off or the user was asleep, is
                // owed a notification just the same, and only the store remembers that (migration 17).
                announce(&state, *account_id).await;
            }
            Err(_) => {
                failed += 1;
                *account_failures.entry(*account_id).or_insert(0) += 1;
            }
        }
    }
    // Judge the sweep on what it actually *tried* — an account we deliberately skipped isn't a
    // failure, and shouldn't drag the whole schedule into backoff.
    sweep_verdict(changed, failed, tried)
}

/// Tell the user about the mail this account is owed a notification for.
///
/// The decisions are pure (`notify.rs`); this is the glue that reads the settings, asks the store what
/// is owed, and — **only after the notification is actually raised** — records that the debt is
/// settled. That order matters: a crash between the two costs a repeated notification, and the other
/// order costs a silently swallowed one. Only one of those loses mail.
pub(crate) async fn announce(state: &AppState, account_id: i64) {
    // How many are really owed — and the newest of them. The COUNT comes from the store, never from
    // the handful of messages we sample for the senders' names: telling the user "50 new messages"
    // when 300 arrived, and then telling them "50 new messages" again five minutes later about older
    // mail, is exactly the storm collapsing exists to prevent.
    let Ok((total, Some(max_id))) = crate::ipc::pending_summary(state, account_id).await else {
        return; // nothing owed, or the store is busy — the debt survives either way
    };
    let Ok(sample) = crate::ipc::pending_notifications(state, account_id, NOTIFY_SAMPLE).await
    else {
        return;
    };

    let enabled = crate::ipc::bool_setting(state, "notify", true).await;
    let per_account = crate::ipc::bool_setting(state, &notify::account_key(account_id), true).await;
    let quiet = crate::ipc::string_setting(state, "quiet_hours")
        .await
        .and_then(|raw| QuietHours::parse(&raw))
        .is_some_and(|q| q.contains(local_minutes()));

    match notify::verdict(enabled, per_account, quiet) {
        // Quiet hours: say nothing, and keep owing it. The mail is still there in the morning, and so
        // is the notification — as one collapsed line, not a night's worth of popups.
        Verdict::Hold => {}
        // Switched off: the mail is not owed at all. Keeping the debt would mean that turning
        // notifications on greets the user with every message that arrived while they were off.
        Verdict::Drop => {
            let _ = crate::ipc::settle(state, account_id, max_id).await;
        }
        Verdict::Announce => {
            let Some(n) = notify::summarize(&sample, total as usize, COLLAPSE_AT) else {
                return;
            };
            // On a worker: a D-Bus round trip is blocking work (connect, authenticate, call), and this
            // is an async task on Tauri's runtime. It is also the one call here with no timeout — a
            // notification daemon that is slow to start would otherwise stall a runtime thread.
            let notifier = state.notifier.clone();
            let shown = tauri::async_runtime::spawn_blocking(move || notifier.notify(&n))
                .await
                .is_ok_and(|r| r.is_ok());
            // A desktop with no notification service (a session that hasn't finished starting, say) is
            // not an error the user needs to hear about while reading their mail — but the debt stays
            // owed, so the mail is not lost either: the next sweep tries again.
            if shown {
                // Settled only now, and only up to the message we actually told them about: mail that
                // arrived while the notification was being raised has a higher id and keeps its debt.
                let _ = crate::ipc::settle(state, account_id, max_id).await;
            }
        }
    }
}

/// The wall-clock time of day, in minutes since midnight, in the **user's** timezone — which is the
/// only one "quiet hours" can possibly mean.
fn local_minutes() -> u16 {
    use chrono::Timelike;
    let now = chrono::Local::now();
    (now.hour() * 60 + now.minute()) as u16
}

#[cfg(test)]
mod tests {
    use super::announce;
    use crate::ipc::AppState;
    use geleit_platform::notify::FakeNotifier;
    use geleit_platform::secret::InMemorySecretStore;
    use geleit_store::NewMessage;
    use std::sync::Arc;

    /// A mailbox on disk with one unread, unannounced message in its inbox, and the `AppState` that
    /// reads it. (`announce` needs no `AppHandle`, which is exactly why it is separated from `sweep`.)
    fn mailbox(notifier: Arc<FakeNotifier>) -> (AppState, tempfile::TempDir, i64) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mail.db");
        let db = path.to_string_lossy().into_owned();
        // Seeded through the app's own door: the mailbox is encrypted at rest, and the key lives in the
        // secret store — so the seed and the AppState must share one.
        let secrets: Arc<dyn geleit_platform::secret::SecretStore> =
            Arc::new(InMemorySecretStore::new());
        {
            let store = geleit_engine::localstore::open_store(&db, secrets.as_ref()).expect("open");
            let acc = store.add_account("a@example.com", None).unwrap();
            let inbox = store.upsert_folder(acc, "INBOX").unwrap();
            store
                .upsert_message(
                    acc,
                    inbox,
                    &NewMessage {
                        uid: Some(1),
                        from_name: Some("Alice".to_owned()),
                        subject: Some("Lunch?".to_owned()),
                        owed_notification: true,
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let state = AppState::with_notifier(
            db,
            secrets,
            notifier as Arc<dyn geleit_platform::notify::Notifier>,
        );
        (state, dir, 1)
    }

    async fn still_owed(state: &AppState, account_id: i64) -> i64 {
        crate::ipc::pending_summary(state, account_id)
            .await
            .expect("summary")
            .0
    }

    #[tokio::test]
    async fn new_mail_is_announced_once_and_then_never_again() {
        let fake = Arc::new(FakeNotifier::new());
        let (state, _dir, acc) = mailbox(fake.clone());

        announce(&state, acc).await;
        let sent = fake.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].summary, "Alice");
        assert_eq!(sent[0].body, "Lunch?");
        assert_eq!(still_owed(&state, acc).await, 0, "the debt is settled");

        // A second sweep must not tell them again — the whole point of the durable fact.
        announce(&state, acc).await;
        assert_eq!(fake.sent().len(), 1, "told once, never again");
    }

    #[tokio::test]
    async fn a_notification_the_desktop_refused_is_still_owed() {
        // A session whose notification service hasn't started yet (the first sweep is 30s after boot).
        // Settling the debt here would lose the message: the user is never told, and never can be.
        let fake = Arc::new(FakeNotifier::failing());
        let (state, _dir, acc) = mailbox(fake.clone());

        announce(&state, acc).await;
        assert!(fake.sent().is_empty());
        assert_eq!(
            still_owed(&state, acc).await,
            1,
            "the desktop wouldn't show it, so the user hasn't been told — try again next sweep"
        );
    }

    #[tokio::test]
    async fn quiet_hours_keep_the_debt_and_switching_notifications_off_settles_it() {
        // The two verdicts are wired to opposite store effects, and swapping them would be invisible to
        // every other test: quiet hours would *lose* the night's mail, and switching notifications back
        // on would greet the user with everything they missed.
        let fake = Arc::new(FakeNotifier::new());
        let (state, _dir, acc) = mailbox(fake.clone());

        // Quiet hours, all day: silent — but the mail is still owed, so it is announced in the morning.
        crate::ipc::set_setting_for_test(&state, "quiet_hours", "00:00-23:59").await;
        announce(&state, acc).await;
        assert!(fake.sent().is_empty(), "quiet hours are quiet");
        assert_eq!(
            still_owed(&state, acc).await,
            1,
            "…but the mail is still owed"
        );

        // Switched off: silent, and the debt goes with it — otherwise turning notifications back on
        // greets the user with every message that arrived while they were off.
        crate::ipc::set_setting_for_test(&state, "quiet_hours", "").await;
        crate::ipc::set_setting_for_test(&state, "notify", "0").await;
        announce(&state, acc).await;
        assert!(fake.sent().is_empty());
        assert_eq!(still_owed(&state, acc).await, 0, "not owed at all");
    }

    #[tokio::test]
    async fn an_account_the_user_muted_is_silent_while_the_others_are_not() {
        let fake = Arc::new(FakeNotifier::new());
        let (state, _dir, acc) = mailbox(fake.clone());
        crate::ipc::set_setting_for_test(&state, &crate::notify::account_key(acc), "0").await;

        announce(&state, acc).await;
        assert!(fake.sent().is_empty(), "this mailbox was muted");
        assert_eq!(still_owed(&state, acc).await, 0);
    }
}
