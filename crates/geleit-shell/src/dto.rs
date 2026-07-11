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
    /// How many messages are in this message's conversation (READ-5). `1` = a singleton; the UI shows
    /// a `conversation · N` marker only when `> 1`. Computed over the loaded page in `list_messages`.
    pub thread_count: u32,
}

/// A message opened for reading.
///
/// `is_html` says whether to show the sandboxed `mail://` iframe (S9.2) or the plain-text view. The
/// message body itself is **deliberately not sent to the frontend**: it is served straight to the
/// iframe from its own origin, so hostile HTML never enters the app's document even as a string.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct MessageBodyDto {
    pub id: i64,
    pub subject: String,
    pub from: String,
    pub date: Option<i64>,
    pub plain: Option<String>,
    /// The message has a formatted (HTML) body → render the sandboxed iframe.
    pub is_html: bool,
    /// Remote content was blocked (PRIV-3) → show the cue + "Load images" (PRIV-2).
    pub has_remote: bool,
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
            thread_count: 1, // set for real by `with_thread_counts` over the whole page
        }
    }
}

/// Fill in each message's conversation size (READ-5) by grouping the loaded page with the engine's
/// threader (`in_reply_to` ↔ `message_id`). Done over the page, in the shell, because the frontend
/// can't depend on the engine — it only ever sees the finished count.
pub fn with_thread_counts(headers: &[MessageHeader], dtos: &mut [MessageDto]) {
    let items: Vec<geleit_engine::thread::ThreadItem> = headers
        .iter()
        .map(|h| geleit_engine::thread::ThreadItem {
            message_id: h.message_id.as_deref(),
            in_reply_to: h.in_reply_to.as_deref(),
        })
        .collect();
    for group in geleit_engine::thread::group(&items) {
        let n = group.len() as u32;
        for idx in group {
            if let Some(d) = dtos.get_mut(idx) {
                d.thread_count = n;
            }
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

/// A well-known destination for a move action. Kept as a role, not a folder name, because a server
/// may call its junk folder "Spam" or "Junk", its trash "Trash" or "Deleted", etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRole {
    Inbox,
    Archive,
    Trash,
    Spam,
}

impl FolderRole {
    fn matches(self, name: &str) -> bool {
        let n = name.to_ascii_lowercase();
        match self {
            FolderRole::Inbox => n == "inbox",
            FolderRole::Archive => n == "archive",
            FolderRole::Trash => n == "trash" || n == "deleted",
            FolderRole::Spam => n == "spam" || n == "junk",
        }
    }
}

/// Pick the actual folder name for a role from the account's folders. Pure — unit-tested. Returns
/// `None` if the account has no such folder (the caller then declines the action rather than
/// inventing a destination — inventing one risks moving mail somewhere the server won't accept).
#[must_use]
pub fn resolve_folder(folders: &[String], role: FolderRole) -> Option<&str> {
    folders.iter().find(|f| role.matches(f)).map(String::as_str)
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
    fn thread_counts_group_a_reply_with_its_parent() {
        // Two messages linked by in_reply_to ↔ message_id form one conversation of 2; a third,
        // unlinked, stays a singleton.
        let headers = vec![
            MessageHeader {
                message_id: Some("<a@x>".into()),
                ..blank()
            },
            MessageHeader {
                message_id: Some("<b@x>".into()),
                in_reply_to: Some("<a@x>".into()),
                ..blank()
            },
            MessageHeader {
                message_id: Some("<c@x>".into()),
                ..blank()
            },
        ];
        let mut dtos: Vec<MessageDto> = headers.iter().cloned().map(MessageDto::from).collect();
        with_thread_counts(&headers, &mut dtos);
        assert_eq!(dtos[0].thread_count, 2);
        assert_eq!(dtos[1].thread_count, 2);
        assert_eq!(dtos[2].thread_count, 1);
    }

    fn blank() -> MessageHeader {
        MessageHeader {
            id: 0,
            uid: None,
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
    fn resolve_folder_finds_the_role_by_its_server_specific_name() {
        let folders: Vec<String> = ["INBOX", "Archive", "Junk", "Deleted"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(resolve_folder(&folders, FolderRole::Inbox), Some("INBOX"));
        assert_eq!(
            resolve_folder(&folders, FolderRole::Archive),
            Some("Archive")
        );
        // the account calls it "Junk" / "Deleted" — the role still resolves
        assert_eq!(resolve_folder(&folders, FolderRole::Spam), Some("Junk"));
        assert_eq!(resolve_folder(&folders, FolderRole::Trash), Some("Deleted"));
    }

    #[test]
    fn resolve_folder_declines_when_the_account_has_no_such_folder() {
        let folders = vec!["INBOX".to_string(), "Sent".to_string()];
        // no Archive/Trash/Spam → None, so the caller declines rather than inventing a destination
        assert_eq!(resolve_folder(&folders, FolderRole::Archive), None);
        assert_eq!(resolve_folder(&folders, FolderRole::Trash), None);
        assert_eq!(resolve_folder(&folders, FolderRole::Spam), None);
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
