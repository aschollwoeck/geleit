//! What to tell the user about new mail, and when to say nothing — kept pure, so it is testable
//! without a clock, a desktop or a database. `scheduler.rs` is the glue that acts on it.
//!
//! Everything a notification shows comes from a **stranger**: a sender who chose their own display
//! name, a subject they wrote. So every string here is treated as hostile until it has been through
//! [`clean`].

use geleit_platform::notify::Notification;
use geleit_store::PendingNotification;

/// Above this many messages we stop naming them and say how many there are. A notification per
/// message is what a mail client does when it is not paying attention: come back from lunch to
/// fourteen popups and the fifteenth is the one you needed.
pub(crate) const COLLAPSE_AT: usize = 3;

/// The longest sender or subject we will put on screen. A notification is one or two lines; a subject
/// longer than this tells the user nothing extra, and a sender who has stuffed a paragraph into their
/// display name is not doing it by accident.
const MAX_FIELD: usize = 90;

/// The settings key for one account's notifications. Per account, because one noisy mailbox shouldn't
/// cost the user the notifications of the other — and because a work account you check on a schedule
/// is a different thing from the one your family writes to.
#[must_use]
pub(crate) fn account_key(account_id: i64) -> String {
    format!("notify_account_{account_id}")
}

/// Make a stranger's text safe to show.
///
/// Control characters are stripped, not escaped: a newline in a display name lets a sender forge what
/// looks like a second field of the notification ("Alice Baker\nRe: your password"), and the desktop
/// will happily render it. Length is clamped for the same reason — a very long subject can push the
/// real content off the popup. Whitespace is collapsed, so the result is one tidy line.
#[must_use]
pub(crate) fn clean(raw: &str, max: usize) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in raw.chars() {
        if c.is_control() || c.is_whitespace() {
            space = !out.is_empty(); // collapse runs; never open with a space
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        if out.chars().count() >= max {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out
}

/// The sender as a person would name them: their display name, else their address.
fn sender(p: &PendingNotification) -> String {
    let name = p.from_name.as_deref().unwrap_or_default().trim();
    if name.is_empty() {
        let addr = p.from_addr.as_deref().unwrap_or_default().trim();
        if addr.is_empty() {
            return "Someone".to_owned();
        }
        return clean(addr, MAX_FIELD);
    }
    clean(name, MAX_FIELD)
}

/// The notification for a batch of new mail — or `None` when there is nothing to say.
///
/// One message gets a name and a subject, because that is what tells you whether to put the coffee
/// down. Several get a count and the names, because a stack of popups is noise, and noise is what
/// gets notifications switched off for good (P3).
#[must_use]
pub(crate) fn summarize(
    new: &[PendingNotification],
    total: usize,
    collapse_at: usize,
) -> Option<Notification> {
    // `new` is a *sample* — enough messages to name the senders. `total` is how many there really are,
    // which is the number the user cares about and the one a notification exists to give them.
    if new.is_empty() {
        return None; // nothing sampled — there is nothing we could truthfully say
    }
    match total {
        0 => None,
        1 => Some(Notification {
            summary: sender(&new[0]),
            body: {
                let subject = clean(new[0].subject.as_deref().unwrap_or_default(), MAX_FIELD);
                if subject.is_empty() {
                    "(no subject)".to_owned()
                } else {
                    subject
                }
            },
        }),
        n => {
            let mut names: Vec<String> = Vec::new();
            for p in new {
                let s = sender(p);
                if !names.contains(&s) {
                    names.push(s);
                }
            }
            // "and others" only when there really ARE others — either more senders than we will name,
            // or a sample too small to have seen everyone who wrote. Five messages from Alice alone is
            // not "From Alice, and others"; the count above already says how much mail there is.
            let unseen_senders = new.len() < total;
            let more = names.len() > collapse_at || unseen_senders;
            names.truncate(collapse_at);
            let body = if more {
                format!("From {}, and others", names.join(", "))
            } else {
                format!("From {}", names.join(", "))
            };
            Some(Notification {
                summary: format!("{n} new messages"),
                body,
            })
        }
    }
}

/// Quiet hours, as minutes-since-midnight. `None` = not set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuietHours {
    pub start: u16,
    pub end: u16,
}

impl QuietHours {
    /// Parse `"22:00-07:30"`. Anything that isn't two valid times is `None` — a malformed setting must
    /// mean "no quiet hours", never "silent forever".
    #[must_use]
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let (a, b) = raw.trim().split_once('-')?;
        let (start, end) = (parse_hhmm(a)?, parse_hhmm(b)?);
        if start == end {
            return None; // a zero-length window is not a window
        }
        Some(Self { start, end })
    }

    /// Is `now` (minutes since midnight) inside the window?
    ///
    /// Wraps around midnight, which is the whole point: `22:00-07:00` is the interesting case, and a
    /// naive `start <= now && now < end` is silent exactly when the user is awake.
    #[must_use]
    pub(crate) fn contains(self, now: u16) -> bool {
        // Ends that meet cover the whole day. [`Self::parse`] never produces one — but if a window ever
        // did arrive that way, silence is the safe reading: quiet hours **hold** the mail (it is still
        // announced later), while the noisy reading would interrupt the user all night.
        if self.start < self.end {
            now >= self.start && now < self.end
        } else {
            now >= self.start || now < self.end
        }
    }
}

fn parse_hhmm(s: &str) -> Option<u16> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u16, u16) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// What to do with a batch of pending notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Show it, then record that we have.
    Announce,
    /// Say nothing, and **keep owing it** — quiet hours end, and the user should still learn that mail
    /// came while they slept (as one collapsed notification, not a night's worth of popups).
    Hold,
    /// Say nothing, and settle the debt: notifications are off, so this mail is not owed. Otherwise
    /// switching them on would dump every message received while they were off.
    Drop,
}

/// Should this batch be announced now?
#[must_use]
pub(crate) fn verdict(enabled: bool, account_enabled: bool, quiet_now: bool) -> Verdict {
    if !enabled || !account_enabled {
        return Verdict::Drop;
    }
    if quiet_now {
        return Verdict::Hold;
    }
    Verdict::Announce
}

#[cfg(test)]
mod tests {
    use super::{account_key, clean, summarize, verdict, QuietHours, Verdict, COLLAPSE_AT};
    use geleit_store::PendingNotification;

    fn msg(from_name: Option<&str>, from_addr: Option<&str>, subject: &str) -> PendingNotification {
        PendingNotification {
            id: 1,
            from_name: from_name.map(str::to_owned),
            from_addr: from_addr.map(str::to_owned),
            subject: Some(subject.to_owned()),
        }
    }

    #[test]
    fn one_message_is_announced_by_name_and_subject() {
        let n = summarize(
            &[msg(Some("Alice Baker"), None, "Lunch on Thursday?")],
            1,
            COLLAPSE_AT,
        )
        .expect("one message is worth announcing");
        assert_eq!(n.summary, "Alice Baker");
        assert_eq!(n.body, "Lunch on Thursday?");
    }

    #[test]
    fn a_sender_with_no_name_is_shown_by_address_and_one_with_neither_is_still_shown() {
        let n = summarize(
            &[msg(None, Some("alice@example.com"), "Hi")],
            1,
            COLLAPSE_AT,
        )
        .unwrap();
        assert_eq!(n.summary, "alice@example.com");
        // A blank display name must not win over a real address.
        let n = summarize(
            &[msg(Some("   "), Some("bob@example.com"), "Hi")],
            1,
            COLLAPSE_AT,
        )
        .unwrap();
        assert_eq!(n.summary, "bob@example.com");
        // And a message with no sender at all still gets announced — never an empty popup.
        let n = summarize(&[msg(None, None, "")], 1, COLLAPSE_AT).unwrap();
        assert_eq!(n.summary, "Someone");
        assert_eq!(n.body, "(no subject)");
    }

    #[test]
    fn several_messages_collapse_into_one_notification() {
        // The rule that keeps this calm: a popup per message is how a mail client trains you to turn
        // notifications off.
        let batch = [
            msg(Some("Alice"), None, "a"),
            msg(Some("Bob"), None, "b"),
            msg(Some("Cara"), None, "c"),
        ];
        let n = summarize(&batch, batch.len(), COLLAPSE_AT).unwrap();
        assert_eq!(n.summary, "3 new messages");
        assert_eq!(n.body, "From Alice, Bob, Cara");

        // Above the threshold the names stop being useful and the count is the message.
        let many: Vec<_> = ["Alice", "Bob", "Cara", "Dan", "Eve"]
            .iter()
            .map(|n| msg(Some(n), None, "x"))
            .collect();
        let n = summarize(&many, many.len(), COLLAPSE_AT).unwrap();
        assert_eq!(n.summary, "5 new messages");
        assert_eq!(n.body, "From Alice, Bob, Cara, and others");
    }

    #[test]
    fn a_single_sender_who_wrote_five_times_is_never_from_alice_and_others() {
        // There are no others. The threshold is about how many *people* can usefully be named — the
        // count above already says how much mail there is.
        let batch: Vec<_> = (0..5).map(|_| msg(Some("Alice"), None, "x")).collect();
        let n = summarize(&batch, batch.len(), COLLAPSE_AT).unwrap();
        assert_eq!(n.summary, "5 new messages");
        assert_eq!(n.body, "From Alice");
    }

    #[test]
    fn a_backlog_says_how_many_there_really_are_not_how_many_we_looked_at() {
        // We only read a handful of messages to get the senders' names. Reporting *that* as the count
        // would tell a user with 300 waiting that they have 10 — and then settle only those 10, so the
        // next sweep raises another "10 new messages" five minutes later, about older mail. The number
        // is the entire reason for collapsing.
        let sample: Vec<_> = ["Alice", "Bob", "Cara"]
            .iter()
            .map(|n| msg(Some(n), None, "x"))
            .collect();
        let n = summarize(&sample, 312, COLLAPSE_AT).unwrap();
        assert_eq!(n.summary, "312 new messages");
        assert_eq!(
            n.body, "From Alice, Bob, Cara, and others",
            "we didn't see them all, so we can't claim to have named everyone"
        );
    }

    #[test]
    fn one_sender_who_wrote_twice_is_named_once() {
        let batch = [msg(Some("Alice"), None, "a"), msg(Some("Alice"), None, "b")];
        let n = summarize(&batch, batch.len(), COLLAPSE_AT).unwrap();
        assert_eq!(n.summary, "2 new messages");
        assert_eq!(n.body, "From Alice");
    }

    #[test]
    fn nothing_new_says_nothing() {
        assert_eq!(summarize(&[], 0, COLLAPSE_AT), None);
    }

    #[test]
    fn a_hostile_sender_cannot_forge_a_notification() {
        // Everything on a notification was written by a stranger. A newline in a display name would
        // otherwise let them draw what looks like a second line of the popup — and the desktop renders
        // it faithfully.
        let n = summarize(
            &[msg(
                Some("Alice\nYour password has expired"),
                None,
                "Click\rhere",
            )],
            1,
            COLLAPSE_AT,
        )
        .unwrap();
        assert_eq!(n.summary, "Alice Your password has expired");
        assert!(!n.summary.contains('\n'));
        assert_eq!(n.body, "Click here");

        // …and a subject long enough to push the real content off the screen is clamped.
        let long = "x".repeat(500);
        let n = summarize(&[msg(Some("Alice"), None, &long)], 1, COLLAPSE_AT).unwrap();
        assert!(n.body.chars().count() <= 91, "clamped: {}", n.body.len());
        assert!(n.body.ends_with('…'));
    }

    #[test]
    fn clean_collapses_whitespace_and_keeps_the_words() {
        assert_eq!(clean("  Alice   Baker \t ", 90), "Alice Baker");
        assert_eq!(clean("", 90), "");
        assert_eq!(clean("\n\n\n", 90), "");
        assert_eq!(clean("Grüße aus München", 90), "Grüße aus München");
    }

    #[test]
    fn quiet_hours_wrap_around_midnight() {
        // The case that matters — and the one a naive `start <= now < end` gets exactly backwards,
        // going silent all day and loud all night.
        let night = QuietHours::parse("22:00-07:00").unwrap();
        assert!(night.contains(23 * 60));
        assert!(night.contains(0));
        assert!(night.contains(6 * 60 + 59));
        assert!(!night.contains(7 * 60));
        assert!(!night.contains(12 * 60));
        assert!(
            night.contains(22 * 60),
            "the boundary is inclusive at the start"
        );

        // A daytime window doesn't wrap.
        let day = QuietHours::parse("09:00-17:00").unwrap();
        assert!(day.contains(12 * 60));
        assert!(!day.contains(8 * 60));
        assert!(!day.contains(17 * 60), "…and exclusive at the end");
    }

    #[test]
    fn the_per_account_key_is_the_one_the_settings_window_writes() {
        // The frontend builds this key itself (it can't depend on our crates — P4), and the store's
        // `delete_account` deletes it by the same shape. Three places, one string: if they ever drift,
        // the switch the user flips is not the switch the scheduler reads, and a muted account is
        // silently un-muted (or worse, an un-muted one goes silent with no control to find).
        assert_eq!(account_key(7), "notify_account_7");
        assert_ne!(account_key(7), account_key(8));
    }

    #[test]
    fn a_window_whose_ends_meet_covers_the_whole_day() {
        // Unreachable through `parse` (which rejects it), but `contains` must still answer safely:
        // quiet hours HOLD the mail, so the quiet reading costs a delayed notification, while the noisy
        // one would wake the user all night.
        let all_day = QuietHours {
            start: 600,
            end: 600,
        };
        assert!(all_day.contains(600));
        assert!(all_day.contains(0));
        assert!(all_day.contains(1439));
    }

    #[test]
    fn a_malformed_quiet_hours_setting_means_no_quiet_hours_never_silence_forever() {
        for raw in [
            "",
            "22:00",
            "22:00-",
            "24:00-07:00", // 24:00 is not a time of day — midnight is 00:00
            "25:00-07:00",
            "22:60-07:00",
            "abc-def",
            "22:00-22:00",
        ] {
            assert_eq!(QuietHours::parse(raw), None, "{raw:?}");
        }
        assert_eq!(
            QuietHours::parse(" 7:5 - 8:5 "),
            Some(QuietHours {
                start: 7 * 60 + 5,
                end: 8 * 60 + 5
            }),
            "…but a sloppy one that is still unambiguous is honoured"
        );
    }

    #[test]
    fn quiet_hours_hold_the_mail_but_a_switched_off_notification_lets_it_go() {
        // The difference is what happens *later*. Quiet hours end and the user should still learn that
        // mail came while they slept — so the debt is kept. But mail that arrived while notifications
        // were switched off is not owed at all: keeping it would mean switching them on greets you with
        // every message you missed.
        assert_eq!(verdict(true, true, false), Verdict::Announce);
        assert_eq!(verdict(true, true, true), Verdict::Hold);
        assert_eq!(verdict(false, true, false), Verdict::Drop);
        assert_eq!(verdict(false, true, true), Verdict::Drop, "off beats quiet");
        // Per account, because one noisy mailbox shouldn't cost you the notifications of the other.
        assert_eq!(verdict(true, false, false), Verdict::Drop);
    }
}
