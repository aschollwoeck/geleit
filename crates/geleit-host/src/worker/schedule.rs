//! The background scheduler's **decisions**, kept pure so they can be tested without a clock, a
//! network, or a Tauri handle. `scheduler.rs` is the glue that acts on them.
//!
//! Everything here answers one of three questions: how long to wait, whether a sweep counts as a
//! failure, and whether a particular account is worth trying this time round.

use std::time::Duration;

/// How often to look for new mail. Modest by design: this is a poll, and every tick costs a TLS
/// handshake per account. (IMAP IDLE would make it instant — a later slice.)
pub(crate) const INTERVAL: Duration = Duration::from_secs(5 * 60);

/// The longest we ever wait between sweeps, so a laptop that's been shut for a week still checks in
/// promptly after waking.
pub(crate) const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);

/// The wait after `consecutive_failures` failed sweeps.
///
/// A poller that hammers a server it can't reach is rude and pointless — most failures are "the
/// laptop is offline", which lasts minutes. So the delay doubles: 5m → 10m → 20m → 30m (cap). One
/// success resets it.
#[must_use]
pub(crate) fn backoff(consecutive_failures: u32) -> Duration {
    // `1 << n` overflows for n ≥ 32; saturate rather than panic on an absurd streak.
    let factor = 1u32.checked_shl(consecutive_failures).unwrap_or(u32::MAX);
    INTERVAL.saturating_mul(factor).min(MAX_BACKOFF)
}

/// Did a sweep succeed, and if so how many messages are worth announcing?
///
/// A sweep is only a **failure** when *every* account failed — which almost certainly means the
/// problem is us (we're offline), not them. One account with a dead password must not push the whole
/// schedule into backoff and stall everyone else's mail.
pub(crate) fn sweep_verdict(news: usize, failed: usize, total: usize) -> Result<usize, ()> {
    if total == 0 {
        return Ok(0); // no accounts set up yet — nothing to do, but not a failure
    }
    if failed == total {
        return Err(());
    }
    Ok(news)
}

/// Should we try this account on the sweep at `tick`, given how many times in a row it has failed?
///
/// Per-account, because "unreachable" and "wrong password" look identical from here but behave very
/// differently: a network blip clears in minutes, while a revoked password never clears — and a
/// client that retries a failing login every five minutes, unattended, for days, is how providers
/// decide to lock an account or block an IP. So a failing account is tried progressively less often
/// (every 2nd, 4th, then 8th sweep), while healthy accounts keep syncing normally.
#[must_use]
pub(crate) fn should_try(tick: u64, consecutive_failures: u32) -> bool {
    if consecutive_failures == 0 {
        return true;
    }
    // Every 2^n-th sweep, capped at every 8th (~40 min at the default interval) so a recovered
    // account is never stranded for long.
    let every = 1u64 << consecutive_failures.min(3);
    tick.is_multiple_of(every)
}

#[cfg(test)]
mod tests {
    use super::{backoff, should_try, sweep_verdict, INTERVAL, MAX_BACKOFF};

    #[test]
    fn backoff_doubles_per_failure_and_is_capped() {
        assert_eq!(backoff(0), INTERVAL); // healthy: just the interval
        assert_eq!(backoff(1), INTERVAL * 2); // don't hammer a server we can't reach
        assert_eq!(backoff(2), INTERVAL * 4);
        assert_eq!(backoff(3), MAX_BACKOFF); // …but cap it, so a woken laptop checks in promptly
        assert_eq!(backoff(99), MAX_BACKOFF);
        assert_eq!(backoff(u32::MAX), MAX_BACKOFF); // no overflow panic on an absurd streak
    }

    #[test]
    fn a_sweep_fails_only_when_every_account_failed() {
        // The whole point: one dead account must not stall everyone else's mail.
        assert_eq!(sweep_verdict(3, 1, 2), Ok(3)); // one of two failed → still a success
        assert_eq!(sweep_verdict(0, 1, 2), Ok(0)); // …even with no new mail
        assert_eq!(sweep_verdict(0, 2, 2), Err(())); // all failed → probably us → back off
        assert_eq!(sweep_verdict(0, 1, 1), Err(())); // the single-account case
        assert_eq!(sweep_verdict(5, 0, 3), Ok(5)); // all fine
        assert_eq!(sweep_verdict(0, 0, 0), Ok(0)); // no accounts yet: nothing to do, not a failure
    }

    #[test]
    fn a_failing_account_is_retried_ever_less_often_but_never_abandoned() {
        // Healthy → every sweep.
        assert!((0..8).all(|tick| should_try(tick, 0)));

        // Failing once → every 2nd sweep; twice → every 4th; three or more → every 8th, and it stays
        // there (a revoked password mustn't be hammered, but a fixed one must recover on its own).
        let tried = |failures| (0..24).filter(|t| should_try(*t, failures)).count();
        assert_eq!(tried(1), 12);
        assert_eq!(tried(2), 6);
        assert_eq!(tried(3), 3);
        assert_eq!(tried(9), 3, "the retry interval is capped, never 'never'");
        assert_eq!(tried(u32::MAX), 3, "no shift overflow");
    }
}
