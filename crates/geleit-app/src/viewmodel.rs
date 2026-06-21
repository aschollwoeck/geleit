//! Pure mapping from `geleit-store` rows to display values for the UI. Kept Slint-free so it is
//! unit- and mutation-tested; `main.rs` converts these into Slint model items.

use chrono::DateTime;
use geleit_store::{MessageHeader, StoredBody};

/// Display-ready fields for one message-list row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageVm {
    pub sender: String,
    pub subject: String,
    pub snippet: String,
    pub date: String,
    pub unread: bool,
    pub attachment: bool,
    pub starred: bool,
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
        starred: h.flagged,
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

/// The plaintext to show in the reading pane, with honest placeholders when there's no plaintext.
/// HTML rendering is M3, so HTML-only messages get a note rather than raw/escaped markup.
pub fn body_display(body: Option<&StoredBody>) -> String {
    match body {
        None => "(Body not downloaded yet.)".to_owned(),
        Some(b) => match (&b.plain, &b.html) {
            (Some(plain), _) => plain.clone(),
            (None, Some(_)) => "(HTML message — safe rendering arrives in M3.)".to_owned(),
            (None, None) => "(No text content.)".to_owned(),
        },
    }
}

/// A one-line attachment label for the reading pane: `"name · 12.3 KB"`.
pub fn attachment_label(filename: Option<&str>, size: u64) -> String {
    format!("{} · {}", filename.unwrap_or("(unnamed)"), human_size(size))
}

/// The address token currently being typed in a recipients field — the part after the last comma or
/// semicolon, trimmed. Used to drive autocomplete (SEND-9).
pub fn last_token(field: &str) -> &str {
    field.rsplit([',', ';']).next().unwrap_or("").trim()
}

/// Replace the last token in `field` with the chosen `addr`, leaving a trailing ", " so the next
/// recipient can be typed.
pub fn complete_last_token(field: &str, addr: &str) -> String {
    match field.rsplit_once([',', ';']) {
        Some((head, _)) => format!("{}, {addr}, ", head.trim_end()),
        None => format!("{addr}, "),
    }
}

/// Find a special folder by name among `folders` — an exact (case-insensitive) match on any keyword
/// wins; otherwise the first folder whose name contains a keyword. Used to locate Archive / Trash /
/// Junk (ORG-1/2/5), whose exact names vary by provider.
pub fn find_folder<'a>(folders: &'a [String], keywords: &[&str]) -> Option<&'a str> {
    if let Some(f) = folders
        .iter()
        .find(|f| keywords.iter().any(|k| f.eq_ignore_ascii_case(k)))
    {
        return Some(f);
    }
    folders
        .iter()
        .find(|f| {
            let l = f.to_lowercase();
            keywords.iter().any(|k| l.contains(k))
        })
        .map(String::as_str)
}

/// Human-readable byte size (B / KB / MB, 1 decimal above 1 KB).
fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        attachment_label, body_display, complete_last_token, format_date, human_size, last_token,
        message_vm,
    };

    #[test]
    fn find_folder_exact_then_contains() {
        use super::find_folder;
        let folders = vec![
            "INBOX".to_owned(),
            "Deleted Items".to_owned(),
            "Archive".to_owned(),
        ];
        assert_eq!(find_folder(&folders, &["archive"]), Some("Archive")); // exact (ci)
        assert_eq!(
            find_folder(&folders, &["trash", "deleted", "bin"]),
            Some("Deleted Items")
        ); // contains
        assert_eq!(find_folder(&folders, &["junk", "spam"]), None);
    }

    #[test]
    fn last_token_and_complete() {
        assert_eq!(last_token("bo"), "bo");
        assert_eq!(last_token("a@x, bo"), "bo");
        assert_eq!(last_token("a@x ,  ca "), "ca");
        assert_eq!(last_token(""), "");
        assert_eq!(complete_last_token("bo", "bob@y"), "bob@y, ");
        assert_eq!(complete_last_token("a@x, bo", "bob@y"), "a@x, bob@y, ");
        assert_eq!(
            complete_last_token("a@x, b@y, ca", "carol@z"),
            "a@x, b@y, carol@z, "
        );
    }
    use geleit_store::{MessageHeader, StoredBody};

    #[test]
    fn human_size_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn attachment_label_with_and_without_name() {
        assert_eq!(
            attachment_label(Some("note.txt"), 2048),
            "note.txt · 2.0 KB"
        );
        assert_eq!(attachment_label(None, 10), "(unnamed) · 10 B");
    }

    fn header() -> MessageHeader {
        MessageHeader {
            id: 1,
            uid: Some(1),
            message_id: None,
            in_reply_to: None,
            subject: None,
            from_name: None,
            from_addr: None,
            to_addrs: None,
            cc_addrs: None,
            date: None,
            seen: false,
            flagged: false,
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
        h.flagged = true;
        h.subject = Some("Hi".to_owned());
        h.snippet = Some("preview".to_owned());
        let vm = message_vm(&h);
        assert!(!vm.unread);
        assert!(vm.attachment);
        assert!(vm.starred);
        assert_eq!(vm.subject, "Hi");
        assert_eq!(vm.snippet, "preview");
    }

    #[test]
    fn body_display_cases() {
        assert_eq!(body_display(None), "(Body not downloaded yet.)");
        assert_eq!(
            body_display(Some(&StoredBody {
                plain: Some("hello".to_owned()),
                html: Some("<p>hi</p>".to_owned()),
            })),
            "hello"
        );
        assert!(body_display(Some(&StoredBody {
            plain: None,
            html: Some("<p>hi</p>".to_owned()),
        }))
        .contains("HTML message"));
        assert_eq!(
            body_display(Some(&StoredBody {
                plain: None,
                html: None,
            })),
            "(No text content.)"
        );
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
