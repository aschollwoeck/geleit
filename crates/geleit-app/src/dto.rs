//! The IPC data types and the pure mapping from store rows to what the UI shows.
//!
//! Split from [`crate::ipc`] deliberately: the commands there are blocking-store glue whose mutants
//! survive spuriously (cf. `geleit-app/src/refresh.rs`), while everything here is pure and stays
//! **mutation-tested** — the same split as the Slint app's `viewmodel.rs`.
//!
//! These are DTOs, not store types: the frontend never sees `geleit_store` types, so the schema can
//! evolve without breaking the UI, and the UI cannot reach into the store even by accident.
use geleit_store::{DraftContent, DraftRow, MessageHeader};
use serde::{Deserialize, Serialize};

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
    /// Unread count for this folder (0 when none) — shown in the rail.
    pub unread: i64,
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
    /// The account this message belongs to. Only meaningful in the merged "All inboxes" view (where
    /// rows span accounts); `0` in a single-folder listing, where the UI already knows the account.
    pub account: i64,
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
    /// Attachments (name + human-readable size) shown in the reading pane; bytes are fetched on demand
    /// to save (READ-8). Order matches the stored/parsed order, so a row's index is its save key.
    pub attachments: Vec<AttachmentDto>,
}

/// One attachment shown in the reading pane. Metadata only — the bytes live on the server and are
/// fetched when the user chooses to save.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct AttachmentDto {
    pub name: String,
    pub size: String,
}

/// A human-readable byte size (e.g. `540 bytes`, `12.4 KB`, `3.1 MB`). Pure. Uses 1024-based units;
/// bytes stay exact, larger units get one decimal (trimmed if `.0`).
#[must_use]
pub fn human_size(bytes: i64) -> String {
    let b = bytes.max(0) as f64;
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let (value, unit) = if b < KB {
        return format!("{} bytes", bytes.max(0));
    } else if b < MB {
        (b / KB, "KB")
    } else if b < GB {
        (b / MB, "MB")
    } else {
        (b / GB, "GB")
    };
    // Round to one decimal using integer tenths, dropping a trailing `.0` (and avoiding any float
    // equality). `tenths` is small (< 10240), so the i64 cast never truncates meaningfully.
    let tenths = (value * 10.0).round() as i64;
    if tenths % 10 == 0 {
        format!("{} {unit}", tenths / 10)
    } else {
        format!("{}.{} {unit}", tenths / 10, tenths % 10)
    }
}

/// Whether a folder is a well-known special folder that must not be renamed or deleted (ORG-6): the
/// Inbox, the standard role folders, and GeleitMail's local `Saved`/`Drafts`. Exact case-insensitive
/// match on the common names, so ordinary user folders (e.g. `Work`, `Receipts`) stay editable. The
/// UI mirrors this in `view::is_protected_folder`; this copy is the authority the IPC commands
/// re-check, so a rename/delete of a protected folder is refused even if the UI is bypassed.
#[must_use]
pub fn is_protected_folder(name: &str) -> bool {
    matches!(
        name.trim().to_lowercase().as_str(),
        "inbox"
            | "sent"
            | "sent items"
            | "sent mail"
            | "drafts"
            | "draft"
            | "archive"
            | "trash"
            | "deleted"
            | "deleted items"
            | "bin"
            | "spam"
            | "junk"
            | "saved"
    )
}

/// Validate a user-entered folder name (ORG-6): trims surrounding whitespace and rejects an empty
/// name or one containing a path/hierarchy separator (folders are kept flat). Returns the cleaned
/// name. Pure.
///
/// # Errors
/// A calm, user-facing message when the name is blank or contains `/` or `\`.
pub fn validate_folder_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("Enter a folder name.".to_owned());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("A folder name can't contain a slash.".to_owned());
    }
    Ok(name.to_owned())
}

/// A filesystem-safe default name for saving an attachment. Unlike [`safe_filename_stem`] it keeps
/// the extension (dots), only stripping directory separators and control characters (so a hostile
/// `../../etc/passwd` filename can't steer the default save path), capping length and falling back to
/// `attachment` when nothing usable remains. Pure.
#[must_use]
pub fn safe_attachment_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .take(120)
        .collect();
    let cleaned = cleaned.trim().trim_matches(['_', '.', ' ']).trim();
    if cleaned.is_empty() {
        "attachment".to_owned()
    } else {
        cleaned.to_owned()
    }
}

/// A compose form, prefilled for a reply/forward or blank for a new message. Plain strings — the
/// compose window is the app's own document, never a webview; untrusted content never enters it.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ComposeDraft {
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: String,
    /// Threading headers, carried opaquely and passed straight back to `send_message`.
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

/// A resumed draft: the compose form plus the on-disk paths its saved attachments were materialised
/// to (so send / re-save read them through the normal path-based flow, like freshly-picked files).
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct ResumedDraft {
    pub draft: ComposeDraft,
    pub attachments: Vec<String>,
}

/// A row in the Drafts list: enough to recognise a saved draft without loading its whole body.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DraftSummary {
    pub id: i64,
    /// Recipient line as typed (comma-joined), or empty. Shown so drafts to different people differ.
    pub to: String,
    pub subject: String,
    /// A short one-line preview of the body.
    pub snippet: String,
    pub updated_at: i64,
}

/// Map a compose form to the store's draft content (a 1:1 field copy — the two types are deliberately
/// identical, kept separate so the UI DTO and the store schema can evolve independently).
#[must_use]
pub fn draft_content_from(d: &ComposeDraft) -> DraftContent {
    DraftContent {
        to: d.to.clone(),
        cc: d.cc.clone(),
        subject: d.subject.clone(),
        body: d.body.clone(),
        in_reply_to: d.in_reply_to.clone(),
        references: d.references.clone(),
    }
}

/// Rebuild a compose form from a stored draft's content, to resume editing it.
#[must_use]
pub fn compose_from_draft(c: DraftContent) -> ComposeDraft {
    ComposeDraft {
        to: c.to,
        cc: c.cc,
        subject: c.subject,
        body: c.body,
        in_reply_to: c.in_reply_to,
        references: c.references,
    }
}

/// One-line preview for a draft list row: the body with newlines flattened to spaces, trimmed, and
/// clipped to `max` chars on a char boundary (an ellipsis marks a clip). Pure.
#[must_use]
pub fn draft_snippet(body: &str, max: usize) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let clipped: String = flat.chars().take(max).collect();
    format!("{}…", clipped.trim_end())
}

/// A filesystem-safe filename stem from a message subject, for the default `<subject>.eml` save name.
/// Keeps alphanumerics, spaces, `-` and `_`; every other char becomes `_`; capped at 60 chars and
/// trimmed of surrounding whitespace/underscores. Falls back to `"message"` when nothing usable
/// remains. Pure.
#[must_use]
pub fn safe_filename_stem(subject: &str) -> String {
    let stem: String = subject
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    let stem = stem.trim().trim_matches('_').trim();
    if stem.is_empty() {
        "message".to_owned()
    } else {
        stem.to_owned()
    }
}

/// Map a stored draft row into its list summary.
#[must_use]
pub fn draft_summary(row: &DraftRow) -> DraftSummary {
    DraftSummary {
        id: row.id,
        to: row.content.to.clone(),
        subject: row.content.subject.clone(),
        snippet: draft_snippet(&row.content.body, 80),
        updated_at: row.updated_at,
    }
}

/// Format a unix timestamp as a short human date (e.g. `12 Jul 2026`) for a reply's attribution line
/// and a forward's `Date:` header. Pure — Howard Hinnant's civil-from-days (the same exact algorithm
/// the frontend's `view::format_date` uses; the two crates can't share it, as the frontend depends on
/// none of ours). Out-of-range timestamps fall back to the raw value rather than fabricate a date.
fn format_email_date(ts: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let day = ts.div_euclid(86_400);
    if !(-25_567..=47_481).contains(&day) {
        return ts.to_string(); // 1900–2099; anything absurd, don't invent a date
    }
    let z = day + 719_468;
    let era = z / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as usize;
    let y = if m <= 2 { y + 1 } else { y };
    format!("{d} {} {y}", MONTHS[m - 1])
}

/// Build a prefilled compose draft from a stored message, for `kind` ("reply" | "reply_all" |
/// "forward"). Pure — maps the header/body into the engine's `Original`, calls the matching engine
/// builder, and flattens the result into the form DTO. Unit-tested; the engine builders themselves
/// (Re:/Fwd: subjects, quoting, threading) are tested in `geleit-engine`.
pub fn compose_draft_from(
    header: &MessageHeader,
    body_plain: &str,
    my_name: Option<String>,
    my_addr: String,
    kind: &str,
) -> Result<ComposeDraft, String> {
    use geleit_engine::message::{self, Original};
    // Format the date for the quoted attribution / forward header. Passing the raw epoch (`date
    // .to_string()`) would render "On 1752307200, … wrote:".
    let date = header.date.map(format_email_date);
    let orig = Original {
        from_name: header.from_name.as_deref(),
        from_addr: header.from_addr.as_deref().unwrap_or_default(),
        subject: header.subject.as_deref().unwrap_or_default(),
        date: date.as_deref(),
        message_id: header.message_id.as_deref(),
        in_reply_to: header.in_reply_to.as_deref(),
        to: header.to_addrs.as_deref().unwrap_or_default(),
        cc: header.cc_addrs.as_deref().unwrap_or_default(),
        body_text: body_plain,
    };
    let draft = match kind {
        "reply" => message::reply(&orig, my_name, my_addr),
        // reply_all excludes *my* addresses from the recipients, so it must know them.
        "reply_all" => {
            let mine = [my_addr.clone()];
            message::reply_all(&orig, &mine, my_name, my_addr)
        }
        "forward" => message::forward(&orig, my_name, my_addr),
        _ => return Err("Unknown compose action.".to_owned()),
    };
    Ok(ComposeDraft {
        to: draft.to.join(", "),
        cc: draft.cc.join(", "),
        subject: draft.subject,
        body: draft.body_text,
        in_reply_to: draft.in_reply_to,
        references: draft.references,
    })
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
            account: 0,      // set only by the merged "All inboxes" listing
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
    fn is_protected_folder_guards_special_but_not_user_folders() {
        for n in [
            "Inbox", "INBOX", "Sent", "Drafts", "Trash", "Archive", "Junk", " Saved ",
        ] {
            assert!(is_protected_folder(n), "{n} should be protected");
        }
        for n in ["Work", "Receipts", "Sent-2024", "Projects", ""] {
            assert!(!is_protected_folder(n), "{n} should be editable");
        }
    }

    #[test]
    fn validate_folder_name_trims_and_rejects_blank_or_slashes() {
        assert_eq!(validate_folder_name("  Work  ").unwrap(), "Work");
        assert!(validate_folder_name("").is_err());
        assert!(validate_folder_name("   ").is_err());
        assert!(validate_folder_name("a/b").is_err());
        assert!(validate_folder_name("a\\b").is_err());
    }

    #[test]
    fn human_size_scales_units_and_trims_trailing_zero() {
        assert_eq!(human_size(0), "0 bytes");
        assert_eq!(human_size(540), "540 bytes");
        assert_eq!(human_size(1024), "1 KB"); // exact → no decimal
        assert_eq!(human_size(1536), "1.5 KB"); // 1.5 KB
        assert_eq!(human_size(1024 * 1024), "1 MB");
        assert_eq!(human_size(3_250_586), "3.1 MB");
        // Exactly at the MB and GB boundaries promote to the larger unit (pins the `< MB`/`< GB`
        // comparisons — a `<=` there would keep the smaller unit, e.g. "1024 KB").
        assert_eq!(human_size(1024 * 1024 * 1024), "1 GB");
        assert_eq!(human_size(2 * 1024 * 1024 * 1024), "2 GB");
        assert_eq!(human_size(-5), "0 bytes"); // negative clamps to 0
    }

    #[test]
    fn safe_attachment_filename_keeps_extension_but_strips_paths() {
        assert_eq!(safe_attachment_filename("report.pdf"), "report.pdf");
        // Directory separators (traversal) become underscores; extension kept.
        assert_eq!(safe_attachment_filename("../../etc/passwd"), "etc_passwd");
        assert_eq!(safe_attachment_filename("a\\b\\c.txt"), "a_b_c.txt");
        // Control characters are stripped.
        assert_eq!(safe_attachment_filename("na\tme.txt"), "na_me.txt");
        // Nothing usable → the fallback, never an empty filename.
        assert_eq!(safe_attachment_filename("   "), "attachment");
        assert_eq!(safe_attachment_filename("/"), "attachment");
    }

    #[test]
    fn safe_filename_stem_sanitises_caps_and_falls_back() {
        assert_eq!(safe_filename_stem("Q3 report"), "Q3 report");
        // Path separators and other punctuation become underscores.
        assert_eq!(safe_filename_stem("Re: a/b\\c?"), "Re_ a_b_c");
        // Leading/trailing underscores and whitespace are trimmed away.
        assert_eq!(safe_filename_stem("  *hi*  "), "hi");
        // Nothing usable → the fallback stem, never an empty filename.
        assert_eq!(safe_filename_stem("///"), "message");
        assert_eq!(safe_filename_stem(""), "message");
        // Capped at 60 chars.
        assert_eq!(safe_filename_stem(&"a".repeat(100)).len(), 60);
    }

    #[test]
    fn draft_snippet_flattens_whitespace_and_clips_on_a_boundary() {
        assert_eq!(draft_snippet("  hello   world \n", 80), "hello world");
        assert_eq!(draft_snippet("", 80), "");
        // Exactly at the cap: no ellipsis.
        assert_eq!(draft_snippet("abcde", 5), "abcde");
        // Over the cap: clipped with an ellipsis, trailing space trimmed before it.
        assert_eq!(draft_snippet("one two three", 4), "one…");
        // Multibyte chars are clipped by char count, not bytes (no panic mid-char).
        assert_eq!(draft_snippet("héllo wörld", 5), "héllo…");
    }

    #[test]
    fn compose_and_draft_content_round_trip_identically() {
        let d = ComposeDraft {
            to: "a@x.io, b@y.io".to_owned(),
            cc: "c@z.io".to_owned(),
            subject: "Hi".to_owned(),
            body: "Body".to_owned(),
            in_reply_to: Some("<m1@x>".to_owned()),
            references: vec!["<r0@x>".to_owned(), "<r1@x>".to_owned()],
        };
        // ComposeDraft → DraftContent → ComposeDraft is lossless.
        assert_eq!(compose_from_draft(draft_content_from(&d)), d);
    }

    #[test]
    fn draft_summary_carries_id_recipient_subject_and_a_snippet() {
        let row = DraftRow {
            id: 7,
            content: DraftContent {
                to: "a@x.io".to_owned(),
                cc: String::new(),
                subject: "Plan".to_owned(),
                body: "Let's meet\nnext week to plan.".to_owned(),
                in_reply_to: None,
                references: Vec::new(),
            },
            updated_at: 1_700_000_000,
            server_folder: None,
        };
        let s = draft_summary(&row);
        assert_eq!(s.id, 7);
        assert_eq!(s.to, "a@x.io");
        assert_eq!(s.subject, "Plan");
        assert_eq!(s.snippet, "Let's meet next week to plan.");
        assert_eq!(s.updated_at, 1_700_000_000);
    }

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
    fn compose_reply_prefills_sender_and_re_subject() {
        let mut h = blank();
        h.from_addr = Some("alice@example.com".into());
        h.subject = Some("Lunch?".into());
        h.message_id = Some("<m1@x>".into());
        let d =
            compose_draft_from(&h, "See you then", None, "me@example.com".into(), "reply").unwrap();
        assert_eq!(d.to, "alice@example.com");
        assert!(d.subject.starts_with("Re:"), "subject={}", d.subject);
        assert_eq!(d.in_reply_to.as_deref(), Some("<m1@x>")); // threading carried
        assert!(d.body.contains("See you then"), "original quoted");
    }

    #[test]
    fn compose_reply_all_keeps_the_others_but_drops_me() {
        let mut h = blank();
        h.from_addr = Some("alice@example.com".into());
        h.to_addrs = Some("me@example.com, bob@example.com".into());
        h.subject = Some("Plan".into());
        let d = compose_draft_from(&h, "", None, "me@example.com".into(), "reply_all").unwrap();
        assert!(d.to.contains("alice@example.com"));
        assert!(d.to.contains("bob@example.com"));
        assert!(
            !d.to.contains("me@example.com"),
            "my own address is excluded"
        );
    }

    #[test]
    fn email_date_is_human_readable_not_a_raw_epoch() {
        // A table of exact epoch-seconds → date, spanning leap days, century and year boundaries, so
        // the civil-from-days arithmetic is pinned (not just a couple of happy cases).
        let cases = [
            (0_i64, "1 Jan 1970"),         // the epoch
            (5_097_600, "1 Mar 1970"),     // just past a non-leap February
            (951_782_400, "29 Feb 2000"),  // a leap day in a century that IS a leap year
            (951_868_800, "1 Mar 2000"),   // ...and the day after
            (1_704_067_200, "1 Jan 2024"), // a year boundary
            (1_783_771_200, "11 Jul 2026"),
            (-2_208_988_800, "1 Jan 1900"), // the earliest we format
            (4_102_358_400, "31 Dec 2099"), // the latest
        ];
        for (ts, expected) in cases {
            assert_eq!(format_email_date(ts), expected, "ts={ts}");
        }
        // an absurd timestamp falls back to the raw value rather than inventing a date
        assert_eq!(format_email_date(i64::MAX), i64::MAX.to_string());
    }

    #[test]
    fn a_reply_quotes_a_readable_date_not_an_epoch() {
        let mut h = blank();
        h.from_addr = Some("alice@example.com".into());
        h.subject = Some("Hi".into());
        h.date = Some(1_783_771_200);
        let d = compose_draft_from(&h, "hello", None, "me@x".into(), "reply").unwrap();
        assert!(d.body.contains("11 Jul 2026"), "body={}", d.body);
        assert!(
            !d.body.contains("1783771200"),
            "raw epoch leaked: {}",
            d.body
        );
    }

    #[test]
    fn compose_forward_uses_fwd_and_no_recipient() {
        let mut h = blank();
        h.from_addr = Some("alice@example.com".into());
        h.subject = Some("Report".into());
        let d = compose_draft_from(&h, "body", None, "me@example.com".into(), "forward").unwrap();
        assert!(
            d.subject.to_lowercase().starts_with("fwd:"),
            "subject={}",
            d.subject
        );
        assert_eq!(d.to, "", "forward leaves the recipient blank");
    }

    #[test]
    fn compose_rejects_an_unknown_kind() {
        let h = blank();
        assert!(compose_draft_from(&h, "", None, "me@x".into(), "bogus").is_err());
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
