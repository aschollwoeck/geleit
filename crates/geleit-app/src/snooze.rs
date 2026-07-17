//! Snooze preset times (ORG-9), computed in the **user's** local timezone.
//!
//! The store speaks unix timestamps; this turns "Tomorrow" into the right one — the user's tomorrow at
//! 08:00, not UTC's. Kept pure (takes `now`, returns labels + timestamps) so the calendar math is unit-
//! and mutation-tested without a clock; the IPC command supplies `chrono::Local::now()`.

use chrono::{DateTime, Datelike, TimeDelta, TimeZone, Timelike};

/// One offered snooze time: a label to show and the unix timestamp to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset {
    pub label: String,
    pub at: i64,
}

/// The snooze options to offer at `now`, each already in the future (a preset whose time has passed —
/// "This evening" at 22:00 — is dropped, never offered as a no-op). Order is soonest-intent first.
///
/// Generic over the timezone so it can be tested with a fixed clock; the app calls it with `Local`.
pub fn presets<Tz: TimeZone>(now: DateTime<Tz>) -> Vec<Preset> {
    let mut out = Vec::new();
    let mut offer = |label: &str, when: Option<DateTime<Tz>>| {
        if let Some(w) = when {
            if w > now {
                out.push(Preset {
                    label: label.to_owned(),
                    at: w.timestamp(),
                });
            }
        }
    };

    // Later today — a few hours from now.
    offer(
        "Later today",
        now.clone().checked_add_signed(TimeDelta::hours(3)),
    );
    // This evening — today at 18:00 (dropped once it's past).
    offer("This evening", at_time(&now, 18, 0));
    // Tomorrow — tomorrow at 08:00.
    offer(
        "Tomorrow",
        now.clone()
            .checked_add_signed(TimeDelta::days(1))
            .and_then(|d| at_time(&d, 8, 0)),
    );
    // This weekend — the coming Saturday at 08:00. Only Monday–Friday: on the weekend itself it's
    // meaningless (you're in it — offer "Next week" instead).
    if now.weekday().num_days_from_monday() < 5 {
        offer(
            "This weekend",
            days_until(&now, chrono::Weekday::Sat)
                .and_then(|days| now.clone().checked_add_signed(TimeDelta::days(days)))
                .and_then(|d| at_time(&d, 8, 0)),
        );
    }
    // Next week — the coming Monday at 08:00 (always a fresh week: today-is-Monday rolls to the next).
    let to_monday =
        days_until(&now, chrono::Weekday::Mon).map_or(7, |d| if d == 0 { 7 } else { d });
    offer(
        "Next week",
        now.clone()
            .checked_add_signed(TimeDelta::days(to_monday))
            .and_then(|d| at_time(&d, 8, 0)),
    );

    out
}

/// The same calendar day as `dt` but at `h:m:00`. `None` only on a nonexistent local time (a spring-
/// forward gap — never at 08:00/18:00 in any real zone).
fn at_time<Tz: TimeZone>(dt: &DateTime<Tz>, h: u32, m: u32) -> Option<DateTime<Tz>> {
    dt.with_hour(h)?
        .with_minute(m)?
        .with_second(0)?
        .with_nanosecond(0)
}

/// Whole days from `now`'s date to the next occurrence of `target` (0 when today *is* `target`).
fn days_until<Tz: TimeZone>(now: &DateTime<Tz>, target: chrono::Weekday) -> Option<i64> {
    let today = now.weekday().num_days_from_monday() as i64;
    let want = target.num_days_from_monday() as i64;
    Some((want - today).rem_euclid(7))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }
    fn find<'a>(ps: &'a [Preset], label: &str) -> Option<&'a Preset> {
        ps.iter().find(|p| p.label == label)
    }

    #[test]
    fn later_today_is_three_hours_out() {
        let now = at(2026, 7, 15, 10, 0); // a Wednesday
        let p = presets(now);
        assert_eq!(
            find(&p, "Later today").unwrap().at,
            at(2026, 7, 15, 13, 0).timestamp()
        );
    }

    #[test]
    fn this_evening_is_offered_before_six_and_dropped_after() {
        let morning = presets(at(2026, 7, 15, 10, 0));
        assert_eq!(
            find(&morning, "This evening").unwrap().at,
            at(2026, 7, 15, 18, 0).timestamp()
        );
        // At 20:00, 18:00 is in the past — not offered.
        assert!(find(&presets(at(2026, 7, 15, 20, 0)), "This evening").is_none());
    }

    #[test]
    fn tomorrow_is_next_day_at_eight() {
        let p = presets(at(2026, 7, 15, 10, 0));
        assert_eq!(
            find(&p, "Tomorrow").unwrap().at,
            at(2026, 7, 16, 8, 0).timestamp()
        );
    }

    #[test]
    fn this_weekend_is_the_coming_saturday_and_absent_on_the_weekend() {
        // Wednesday the 15th → Saturday the 18th.
        let wed = presets(at(2026, 7, 15, 10, 0));
        assert_eq!(
            find(&wed, "This weekend").unwrap().at,
            at(2026, 7, 18, 8, 0).timestamp()
        );
        // Saturday the 18th → no "This weekend" (we're in it).
        assert!(find(&presets(at(2026, 7, 18, 10, 0)), "This weekend").is_none());
        // Sunday the 19th → likewise none.
        assert!(find(&presets(at(2026, 7, 19, 10, 0)), "This weekend").is_none());
    }

    #[test]
    fn next_week_is_the_coming_monday_and_never_today() {
        // Wednesday the 15th → Monday the 20th.
        let wed = presets(at(2026, 7, 15, 10, 0));
        assert_eq!(
            find(&wed, "Next week").unwrap().at,
            at(2026, 7, 20, 8, 0).timestamp()
        );
        // On a Monday (the 20th) it rolls a full week to the 27th, never "today".
        let mon = presets(at(2026, 7, 20, 10, 0));
        assert_eq!(
            find(&mon, "Next week").unwrap().at,
            at(2026, 7, 27, 8, 0).timestamp()
        );
    }

    #[test]
    fn every_offered_preset_is_in_the_future() {
        // Sweep a full day of start times; nothing offered may be in the past.
        for h in 0..24 {
            let now = at(2026, 7, 15, h, 30);
            for p in presets(now) {
                assert!(
                    p.at > now.timestamp(),
                    "{} at hour {h} is not future",
                    p.label
                );
            }
        }
    }
}
