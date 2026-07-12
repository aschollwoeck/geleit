//! Pure view logic — no DOM, no IPC, no framework. Deliberately separated so it is unit-testable on
//! the **host** target (and mutation-testable), rather than only inside a browser.

/// The range of dates we are willing to display, as days since the epoch: 1900-01-01 to 2099-12-31.
///
/// Mail carries whatever `Date:` header the sender wrote, and a corrupt or hostile message can carry
/// an absurd one. Outside this range we show *nothing* rather than a fabricated date — and, more to
/// the point, [`civil_from_days`] never has to cope with it (`i64::MIN` would overflow its epoch
/// shift, panicking the message list on a malformed email).
const MIN_DAY: i64 = -25_567; // 1900-01-01
const MAX_DAY: i64 = 47_481; // 2099-12-31

/// Days since the epoch → (year, month, day). Howard Hinnant's civil-from-days; exact, and cheap
/// enough that we don't pull a date library into the WASM bundle for four lines of formatting.
///
/// **Precondition:** `day` is within [`MIN_DAY`]..=[`MAX_DAY`], so the shifted value is always
/// positive and the pre-year-0 era branch of the original algorithm is unreachable — and therefore
/// omitted. Callers must go through [`format_date`], which enforces the range.
fn civil_from_days(day: i64) -> (i64, u32, u32) {
    debug_assert!((MIN_DAY..=MAX_DAY).contains(&day), "out of range: {day}");
    let z = day + 719_468; // days since 0000-03-01, always > 0 given the precondition
    let era = z / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// How the message list shows a date: the time of day if it arrived today, otherwise the day and
/// month, and the year too once it's from a different year. `now` is passed in (not read from the
/// clock) so this stays pure and testable. A missing or out-of-range date renders as nothing.
#[must_use]
pub fn format_date(ts: Option<i64>, now: i64) -> String {
    let Some(ts) = ts else {
        return String::new();
    };
    let day = ts.div_euclid(86_400);
    if !(MIN_DAY..=MAX_DAY).contains(&day) {
        return String::new(); // absurd date — say nothing rather than invent something
    }
    let today = now.div_euclid(86_400).clamp(MIN_DAY, MAX_DAY);
    if day == today {
        let secs = ts.rem_euclid(86_400);
        return format!("{:02}:{:02}", secs / 3600, (secs % 3600) / 60);
    }
    let (y, m, d) = civil_from_days(day);
    let (now_y, _, _) = civil_from_days(today);
    let month = MONTHS[(m as usize) - 1];
    if y == now_y {
        format!("{d} {month}")
    } else {
        format!("{d} {month} {y}")
    }
}

/// Trim a preview snippet to one readable line. Cuts on a word boundary where it can, so we don't
/// slice a word in half, and never splits a UTF-8 character.
#[must_use]
pub fn elide(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= max {
        return text;
    }
    let cut: String = text.chars().take(max).collect();
    let cut = match cut.rsplit_once(' ') {
        // Only back off to the word boundary if it doesn't throw away most of the line: a snippet
        // starting with one very long word would otherwise collapse to almost nothing.
        Some((head, _)) if head.chars().count() >= max / 2 => head,
        _ => cut.trim_end(),
    };
    format!("{cut}…")
}

/// The list index to move to for keyboard navigation: from `current` (`None` = nothing selected),
/// step by `delta` (+1 = next, -1 = previous) over `len` items. Clamps at both ends; from nothing, a
/// forward step lands on the first item and a backward step on the last. `None` when the list is empty.
#[must_use]
pub fn nav_index(len: usize, current: Option<usize>, delta: i32) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match current {
        Some(i) => (i as i32 + delta).clamp(0, len as i32 - 1) as usize,
        None if delta >= 0 => 0,
        None => len - 1,
    })
}

/// Split a recipient field into individual addresses for display as chips. Commas and semicolons
/// separate; surrounding whitespace is trimmed and empty entries dropped. (Direct entry only —
/// display names containing a comma are not handled; the backend re-parses the joined string.)
#[must_use]
pub fn split_addrs(s: &str) -> Vec<String> {
    s.split([',', ';'])
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Merge `typed` recipient text into an existing comma-separated field, de-duplicating
/// case-insensitively and preserving order. Used both when a chip is committed and at send time, so
/// a recipient can't end up in the envelope twice.
#[must_use]
pub fn merge_addrs(existing: &str, typed: &str) -> String {
    let mut out = split_addrs(existing);
    for a in split_addrs(typed) {
        if !out.iter().any(|e| e.eq_ignore_ascii_case(&a)) {
            out.push(a);
        }
    }
    out.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-07-11T12:00:00Z (a Saturday).
    const NOW: i64 = 1_783_771_200;

    #[test]
    fn merge_addrs_dedups_case_insensitively_and_keeps_order() {
        assert_eq!(merge_addrs("", "a@x.com"), "a@x.com");
        assert_eq!(merge_addrs("a@x.com", ""), "a@x.com");
        assert_eq!(merge_addrs("a@x.com", "b@y.com"), "a@x.com, b@y.com");
        assert_eq!(merge_addrs("a@x.com", "a@x.com"), "a@x.com"); // exact dup dropped
        assert_eq!(merge_addrs("a@x.com", "A@X.com"), "a@x.com"); // case-insensitive dup
        assert_eq!(
            merge_addrs("a@x.com", "b@y.com, a@x.com"),
            "a@x.com, b@y.com"
        );
    }

    #[test]
    fn split_addrs_separates_trims_and_drops_blanks() {
        assert_eq!(split_addrs(""), Vec::<String>::new());
        assert_eq!(split_addrs("  "), Vec::<String>::new());
        assert_eq!(split_addrs("a@x.com"), ["a@x.com"]);
        assert_eq!(
            split_addrs(" a@x.com , b@y.com ;c@z.com,"),
            ["a@x.com", "b@y.com", "c@z.com"]
        );
    }

    #[test]
    fn nav_index_steps_and_clamps() {
        assert_eq!(nav_index(0, None, 1), None); // empty list
        assert_eq!(nav_index(0, Some(0), -1), None);
        assert_eq!(nav_index(5, None, 1), Some(0)); // from nothing: first going down
        assert_eq!(nav_index(5, None, -1), Some(4)); // from nothing: last going up
        assert_eq!(nav_index(5, Some(2), 1), Some(3)); // next
        assert_eq!(nav_index(5, Some(2), -1), Some(1)); // previous
        assert_eq!(nav_index(5, Some(4), 1), Some(4)); // clamp at the bottom
        assert_eq!(nav_index(5, Some(0), -1), Some(0)); // clamp at the top
    }

    #[test]
    fn today_shows_the_time_of_day() {
        let same_day_0930 = NOW - 2 * 3600 - 30 * 60;
        assert_eq!(format_date(Some(same_day_0930), NOW), "09:30");
        // midnight and the last minute of the day still format as a time, not a date
        let midnight = NOW.div_euclid(86_400) * 86_400;
        assert_eq!(format_date(Some(midnight), NOW), "00:00");
        assert_eq!(format_date(Some(midnight + 86_399), NOW), "23:59");
    }

    #[test]
    fn earlier_this_year_shows_day_and_month_without_the_year() {
        assert_eq!(format_date(Some(NOW - 7 * 86_400), NOW), "4 Jul");
    }

    #[test]
    fn a_previous_year_includes_the_year() {
        assert_eq!(format_date(Some(1_704_067_200), NOW), "1 Jan 2024");
    }

    #[test]
    fn a_missing_date_renders_as_nothing_rather_than_a_lie() {
        assert_eq!(format_date(None, NOW), "");
    }

    /// A corrupt or hostile `Date:` header must not fabricate a date — and must not panic. Before the
    /// range guard, `i64::MIN` overflowed the epoch shift inside `civil_from_days`.
    #[test]
    fn absurd_dates_render_as_nothing_and_never_panic() {
        for ts in [i64::MIN, i64::MAX, -99_999_999_999, 99_999_999_999] {
            assert_eq!(format_date(Some(ts), NOW), "", "ts={ts}");
        }
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        let cases = [
            (-25_567_i64, (1900, 1, 1)), // the earliest date we display
            (0, (1970, 1, 1)),           // the epoch
            (59, (1970, 3, 1)),          // just past a non-leap February
            (11_016, (2000, 2, 29)),     // a leap day, in a century that IS a leap year
            (11_017, (2000, 3, 1)),      // ...and the day after it
            (19_723, (2024, 1, 1)),      // a year boundary
            (20_645, (2026, 7, 11)),     // today
            (47_481, (2099, 12, 31)),    // the latest date we display
        ];
        for (day, expected) in cases {
            assert_eq!(civil_from_days(day), expected, "day={day}");
        }
    }

    #[test]
    fn elide_leaves_short_text_alone_and_collapses_whitespace() {
        assert_eq!(elide("hello there", 40), "hello there");
        assert_eq!(elide("hello\n\n  there", 40), "hello there");
        // exactly at the limit is not elided
        assert_eq!(elide("abcde", 5), "abcde");
        assert_eq!(elide("abcdef", 5), "abcde…");
    }

    #[test]
    fn elide_cuts_on_a_word_boundary() {
        assert_eq!(elide("the quick brown fox jumps", 16), "the quick brown…");
    }

    /// A single long word can't be cut on a boundary — it must still be cut, not returned whole.
    #[test]
    fn elide_hard_cuts_a_word_too_long_to_break() {
        assert_eq!(
            elide("supercalifragilisticexpialidocious", 10),
            "supercalif…"
        );
    }

    /// The word-boundary backoff must NOT apply when it would throw away most of the line: a snippet
    /// that opens with one very long word would otherwise collapse to almost nothing.
    #[test]
    fn elide_ignores_a_word_boundary_that_would_gut_the_line() {
        // cut = "ab cdefghijkl"; backing off to "ab" (2 chars) is far short of max/2 (6) — so the
        // hard cut wins and we keep a useful preview.
        assert_eq!(elide("ab cdefghijklmnop qrs", 13), "ab cdefghijkl…");
        // whereas a boundary past the halfway mark IS honoured
        assert_eq!(elide("abcdefg hijklmnop", 13), "abcdefg…");
    }

    /// Multi-byte characters must not be sliced mid-character (a byte-index cut would panic).
    #[test]
    fn elide_never_splits_a_utf8_character() {
        let out = elide("grüße über größe straße wörter", 12);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 13);
    }
}
