//! The IPC data types and the pure mapping from store rows to what the UI shows.
//!
//! Split from [`crate::ipc`] deliberately: the commands there are blocking-store glue whose mutants
//! survive spuriously (cf. `geleit-app/src/refresh.rs`), while everything here is pure and stays
//! **mutation-tested** — the same split as the Slint app's `viewmodel.rs`.
//!
//! These are DTOs, not store types: the frontend never sees `geleit_store` types, so the schema can
//! evolve without breaking the UI, and the UI cannot reach into the store even by accident.
use geleit_store::MessageHeader;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AccountDto {
    pub id: i64,
    pub email: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FolderDto {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MessageDto {
    pub id: i64,
    pub subject: String,
    /// Best display name for the sender: the display name if present, else the bare address.
    pub from: String,
    pub snippet: String,
    /// Unix seconds; `None` if the message carried no parseable date.
    pub date: Option<i64>,
    pub seen: bool,
    pub flagged: bool,
    pub has_attachments: bool,
}

/// A message opened for reading. `html` is carried **unrendered** — S9.1 shows only `plain`; S9.2
/// adds the sandboxed iframe. Shipping the field now means the seam does not change in S9.2.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct MessageBodyDto {
    pub id: i64,
    pub subject: String,
    pub from: String,
    pub date: Option<i64>,
    pub plain: Option<String>,
    pub html: Option<String>,
}

/// Sender as the list should show it: display name, else the address, else a calm placeholder.
/// Pure — unit-tested.
#[must_use]
pub fn display_sender(from_name: Option<&str>, from_addr: Option<&str>) -> String {
    let name = from_name.map(str::trim).filter(|s| !s.is_empty());
    let addr = from_addr.map(str::trim).filter(|s| !s.is_empty());
    name.or(addr).unwrap_or("(unknown sender)").to_owned()
}

/// A message with no subject still needs a readable row. Pure — unit-tested.
#[must_use]
pub fn display_subject(subject: Option<&str>) -> String {
    subject
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(no subject)")
        .to_owned()
}

impl From<MessageHeader> for MessageDto {
    fn from(h: MessageHeader) -> Self {
        Self {
            id: h.id,
            subject: display_subject(h.subject.as_deref()),
            from: display_sender(h.from_name.as_deref(), h.from_addr.as_deref()),
            snippet: h.snippet.unwrap_or_default(),
            date: h.date,
            seen: h.seen,
            flagged: h.flagged,
            has_attachments: h.has_attachments,
        }
    }
}

/// Folders in the order the rail shows them: Inbox first, then the other well-known folders, then
/// everything else alphabetically. Pure — unit-tested. (Mirrors the ordering the Slint app used, so
/// the migration doesn't silently reshuffle the user's rail.)
#[must_use]
pub fn folder_rank(name: &str) -> u8 {
    match name.to_ascii_lowercase().as_str() {
        "inbox" => 0,
        "drafts" => 1,
        "sent" => 2,
        "archive" => 3,
        "spam" | "junk" => 4,
        "trash" | "deleted" => 5,
        "saved" => 6,
        _ => 7,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sender_prefers_the_display_name_then_the_address() {
        assert_eq!(display_sender(Some("Ada"), Some("a@x.io")), "Ada");
        assert_eq!(display_sender(None, Some("a@x.io")), "a@x.io");
        // a blank name must not win over a real address
        assert_eq!(display_sender(Some("   "), Some("a@x.io")), "a@x.io");
        assert_eq!(display_sender(None, None), "(unknown sender)");
    }

    #[test]
    fn subject_falls_back_when_missing_or_blank() {
        assert_eq!(display_subject(Some("Hello")), "Hello");
        assert_eq!(display_subject(Some("  ")), "(no subject)");
        assert_eq!(display_subject(None), "(no subject)");
    }

    /// Every well-known folder must have its OWN place in the order — pinned individually, so that
    /// dropping any one of them (and letting it fall through to "custom folder") is caught.
    #[test]
    fn every_well_known_folder_has_a_distinct_place_in_the_order() {
        let order = [
            "Inbox",
            "Drafts",
            "Sent",
            "Archive",
            "Spam",
            "Trash",
            "Saved",
            "Zzz custom",
        ];
        let ranks: Vec<u8> = order.iter().map(|n| folder_rank(n)).collect();
        assert!(
            ranks.windows(2).all(|w| w[0] < w[1]),
            "ranks must be strictly increasing across {order:?}, got {ranks:?}"
        );
    }

    /// The aliases really are aliases — Junk *is* Spam, Deleted *is* Trash.
    #[test]
    fn folder_aliases_share_their_rank() {
        assert_eq!(folder_rank("Junk"), folder_rank("Spam"));
        assert_eq!(folder_rank("Deleted"), folder_rank("Trash"));
    }

    #[test]
    fn inbox_ranks_first_and_unknown_folders_last() {
        assert_eq!(folder_rank("INBOX"), 0);
        assert!(folder_rank("Inbox") < folder_rank("Sent"));
        assert!(folder_rank("Sent") < folder_rank("Trash"));
        assert!(folder_rank("Trash") < folder_rank("Some custom folder"));
        // case-insensitive, and Junk is Spam
        assert_eq!(folder_rank("junk"), folder_rank("Spam"));
    }
}
