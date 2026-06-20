//! Pure mapping from `geleit-store` rows to display values for the UI. Kept Slint-free so it is
//! unit- and mutation-tested; `main.rs` converts these into Slint model items.

use chrono::DateTime;
use geleit_store::MessageHeader;

/// Display-ready fields for one message-list row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageVm {
    pub sender: String,
    pub subject: String,
    pub snippet: String,
    pub date: String,
    pub unread: bool,
    pub attachment: bool,
}

/// Map a stored message header to its display row, applying sensible fallbacks.
pub fn message_vm(h: &MessageHeader) -> MessageVm {
    let sender = h
        .from_name
        .clone()
        .or_else(|| h.from_addr.clone())
        .unwrap_or_else(|| "(unknown sender)".to_owned());
    MessageVm {
        sender,
        subject: h
            .subject
            .clone()
            .unwrap_or_else(|| "(no subject)".to_owned()),
        snippet: h.snippet.clone().unwrap_or_default(),
        date: format_date(h.date),
        unread: !h.seen,
        attachment: h.has_attachments,
    }
}

/// Format a unix-seconds timestamp as a short, locale-agnostic label (`"%b %e, %H:%M"`), or `""`
/// if absent/unparseable. Deterministic (no "now"), so it's testable; relative dates are later polish.
pub fn format_date(secs: Option<i64>) -> String {
    match secs.and_then(|s| DateTime::from_timestamp(s, 0)) {
        Some(dt) => dt.format("%b %e, %H:%M").to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{format_date, message_vm};
    use geleit_store::MessageHeader;

    fn header() -> MessageHeader {
        MessageHeader {
            id: 1,
            uid: Some(1),
            subject: None,
            from_name: None,
            from_addr: None,
            date: None,
            seen: false,
            has_attachments: false,
            snippet: None,
        }
    }

    #[test]
    fn fallbacks_when_fields_absent() {
        let vm = message_vm(&header());
        assert_eq!(vm.sender, "(unknown sender)");
        assert_eq!(vm.subject, "(no subject)");
        assert_eq!(vm.snippet, "");
        assert_eq!(vm.date, "");
        assert!(vm.unread); // seen=false → unread
        assert!(!vm.attachment);
    }

    #[test]
    fn prefers_name_then_addr() {
        let mut h = header();
        h.from_addr = Some("a@example.com".to_owned());
        assert_eq!(message_vm(&h).sender, "a@example.com");
        h.from_name = Some("Anna".to_owned());
        assert_eq!(message_vm(&h).sender, "Anna");
    }

    #[test]
    fn maps_seen_and_attachment_and_fields() {
        let mut h = header();
        h.seen = true;
        h.has_attachments = true;
        h.subject = Some("Hi".to_owned());
        h.snippet = Some("preview".to_owned());
        let vm = message_vm(&h);
        assert!(!vm.unread);
        assert!(vm.attachment);
        assert_eq!(vm.subject, "Hi");
        assert_eq!(vm.snippet, "preview");
    }

    #[test]
    fn date_format_is_deterministic_and_empty_when_absent() {
        assert_eq!(format_date(None), "");
        // 2021-01-01 00:00:00 UTC
        assert_eq!(format_date(Some(1_609_459_200)), "Jan  1, 00:00");
        // out-of-range timestamps don't panic — they yield ""
        assert_eq!(format_date(Some(i64::MAX)), "");
        assert_eq!(format_date(Some(i64::MIN)), "");
    }
}
