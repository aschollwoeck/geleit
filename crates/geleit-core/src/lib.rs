//! `geleit-core` — UI-agnostic domain types shared across the engine.
//!
//! Scaffold placeholder (slice S0.1). Real domain types (Account, Mailbox, Message, …)
//! arrive in later slices. This crate must never depend on UI code (constitution P4,
//! ADR-0003).

/// Returns `true` if `addr` is a syntactically plausible email address.
///
/// Placeholder validation for the scaffold; real address/MIME handling comes later.
/// Kept deliberately small so it is a meaningful mutation-testing target.
#[must_use]
pub fn looks_like_email(addr: &str) -> bool {
    match addr.find('@') {
        Some(at) => at > 0 && at < addr.len() - 1 && !addr.contains(' '),
        None => false,
    }
}

/// What a folder is *for* — the one thing about a folder that isn't just its name.
///
/// The name is the user's (and their provider's, and their language's): GMX calls its drafts folder
/// `Entwürfe`, Gmail calls its sent mail `[Gmail]/Sent Mail`. So a client that decides "this is the
/// Drafts folder" by matching the English word **is wrong for most of the world** — it saves sent mail
/// nowhere, archives into thin air, and shows drafts it can't find.
///
/// Servers already tell us, in the LIST response: RFC 6154 SPECIAL-USE marks each folder with
/// `\Drafts`, `\Sent`, `\Trash`, `\Archive`, `\Junk`. That is the authority. The English-name match
/// stays only as a **fallback**, for the servers that don't advertise it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FolderRole {
    Inbox,
    Drafts,
    Sent,
    Archive,
    Junk,
    Trash,
}

impl FolderRole {
    /// The role as stored in the database and passed to the frontend. Stable — it is persisted.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            FolderRole::Inbox => "inbox",
            FolderRole::Drafts => "drafts",
            FolderRole::Sent => "sent",
            FolderRole::Archive => "archive",
            FolderRole::Junk => "junk",
            FolderRole::Trash => "trash",
        }
    }

    /// Read a role back from its stored key. Unknown keys (a newer schema, a corrupt row) are `None`
    /// rather than a panic — a folder with no role we understand is just a folder.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "inbox" => Some(FolderRole::Inbox),
            "drafts" => Some(FolderRole::Drafts),
            "sent" => Some(FolderRole::Sent),
            "archive" => Some(FolderRole::Archive),
            "junk" => Some(FolderRole::Junk),
            "trash" => Some(FolderRole::Trash),
            _ => None,
        }
    }

    /// Does this folder's **name** say it has this role? The fallback for servers that don't advertise
    /// SPECIAL-USE — English names only, which is exactly the limitation SPECIAL-USE exists to remove.
    ///
    /// Matched on the last segment of the path, so `INBOX.Drafts` and `[Gmail]/Sent Mail` resolve while
    /// `INBOX.Alte Drafts` (an archive that merely *mentions* drafts) does not. Never a substring: the
    /// folder picked here is treated as the real thing — mail is moved into it, drafts are expunged from
    /// it — so a wrong guess is worse than no guess.
    #[must_use]
    pub fn matches_name(self, name: &str) -> bool {
        let leaf = name
            .rsplit(['/', '.'])
            .next()
            .unwrap_or(name)
            .trim()
            .to_ascii_lowercase();
        // INBOX is the one name IMAP itself reserves (RFC 3501), case-insensitively.
        match self {
            FolderRole::Inbox => name.eq_ignore_ascii_case("inbox"),
            // Only the plural. A folder called `Draft` is somebody's own folder — and the drafts folder
            // is *hidden from the rail* and has its whole contents listed as deletable drafts, so
            // claiming one on a guess is how you make a user's mail vanish.
            FolderRole::Drafts => leaf == "drafts",
            FolderRole::Sent => matches!(
                leaf.as_str(),
                // `Sent Messages` is what Apple Mail creates, so any account it has ever touched has
                // one. Missing it means the copy of every sent message is silently saved nowhere —
                // which is the bug this whole feature exists to fix.
                "sent" | "sent items" | "sent mail" | "sent messages" | "sentitems" | "sentmail"
            ),
            FolderRole::Archive => leaf == "archive" || leaf == "archives",
            FolderRole::Junk => leaf == "junk" || leaf == "spam",
            FolderRole::Trash => {
                matches!(leaf.as_str(), "trash" | "deleted" | "deleted items" | "bin")
            }
        }
    }

    /// Every role, for callers that must ask "does this folder have *any* role?" — the rail's
    /// protection guard, for one. A list that drifts from the roles is how a folder the app files mail
    /// into stays renamable.
    pub const ALL: [FolderRole; 6] = [
        FolderRole::Inbox,
        FolderRole::Drafts,
        FolderRole::Sent,
        FolderRole::Archive,
        FolderRole::Junk,
        FolderRole::Trash,
    ];

    /// Does this folder hold a special role — either because the server said so, or because its name
    /// says so? The one answer to "is this folder the app's to manage, not the user's to rename".
    #[must_use]
    pub fn of(name: &str, role: Option<&str>) -> Option<FolderRole> {
        role.and_then(FolderRole::from_key)
            .or_else(|| FolderRole::ALL.into_iter().find(|r| r.matches_name(name)))
    }
}

/// Pick one role from the several a folder may be flagged with (RFC 6154 allows more than one, and
/// Dovecot's `special_use` takes a list). Fixed priority, so the answer can't depend on the order the
/// server happened to send them in — `(\Sent \Archive)` and `(\Archive \Sent)` must not resolve to
/// different folders for "where does sent mail go".
#[must_use]
pub fn pick_role(candidates: &[FolderRole]) -> Option<FolderRole> {
    FolderRole::ALL.into_iter().find(|r| candidates.contains(r))
}

/// Which of an account's folders holds this role?
///
/// **The server's own word first.** A folder the provider marked `\Drafts` is the drafts folder even if
/// it's called `Entwürfe`, `Brouillons`, or `下書き`. Only if no folder carries the role do we fall back
/// to the English name — and if that finds nothing either, `None`: the caller declines the action rather
/// than inventing a destination, because moving someone's mail into a folder we guessed at is worse
/// than telling them we can't.
///
/// Takes `(name, role)` pairs so it stays pure and testable without a database.
#[must_use]
pub fn pick_folder(folders: &[(String, Option<FolderRole>)], role: FolderRole) -> Option<&str> {
    folders
        .iter()
        .find(|(_, r)| *r == Some(role))
        .or_else(|| folders.iter().find(|(n, _)| role.matches_name(n)))
        .map(|(n, _)| n.as_str())
}

#[cfg(test)]
mod folder_role_tests {
    use super::{pick_folder, pick_role, FolderRole};

    fn f(name: &str, role: Option<FolderRole>) -> (String, Option<FolderRole>) {
        (name.to_owned(), role)
    }

    #[test]
    fn the_servers_own_word_beats_the_english_name() {
        // The case this whole thing exists for: a German provider. Nothing here is called "Drafts",
        // and a name-matching client finds nothing at all — no drafts list, no draft sync.
        let gmx = [
            f("INBOX", None),
            f("Entwürfe", Some(FolderRole::Drafts)),
            f("Gesendet", Some(FolderRole::Sent)),
            f("Papierkorb", Some(FolderRole::Trash)),
        ];
        assert_eq!(pick_folder(&gmx, FolderRole::Drafts), Some("Entwürfe"));
        assert_eq!(pick_folder(&gmx, FolderRole::Sent), Some("Gesendet"));
        assert_eq!(pick_folder(&gmx, FolderRole::Trash), Some("Papierkorb"));
        assert_eq!(pick_folder(&gmx, FolderRole::Archive), None, "it has none");
    }

    #[test]
    fn a_folder_flagged_by_the_server_wins_over_one_merely_named_for_the_role() {
        // A user folder literally called "Drafts" next to the real (flagged) one. The server's mark is
        // the authority — otherwise we'd expunge drafts from the wrong mailbox.
        let folders = [
            f("Drafts", None),
            f("INBOX.Entwürfe", Some(FolderRole::Drafts)),
        ];
        assert_eq!(
            pick_folder(&folders, FolderRole::Drafts),
            Some("INBOX.Entwürfe")
        );
    }

    #[test]
    fn without_special_use_the_english_name_still_works() {
        // Servers that don't advertise SPECIAL-USE (and every account synced before this landed, until
        // its next folder sync) must keep working exactly as before.
        let plain = [
            f("INBOX", None),
            f("INBOX.Drafts", None),
            f("[Gmail]/Sent Mail", None),
            f("Deleted Items", None),
        ];
        assert_eq!(
            pick_folder(&plain, FolderRole::Drafts),
            Some("INBOX.Drafts")
        );
        assert_eq!(
            pick_folder(&plain, FolderRole::Sent),
            Some("[Gmail]/Sent Mail")
        );
        assert_eq!(
            pick_folder(&plain, FolderRole::Trash),
            Some("Deleted Items")
        );
        assert_eq!(pick_folder(&plain, FolderRole::Inbox), Some("INBOX"));
        assert_eq!(pick_folder(&plain, FolderRole::Junk), None);
    }

    #[test]
    fn a_folder_that_merely_mentions_the_word_is_never_picked() {
        // "Alte Drafts" is someone's archive of old drafts. Picking it would hide it from the rail and
        // expunge its contents as though they were drafts. No guess is better than a wrong one.
        let folders = [f("INBOX", None), f("INBOX.Alte Drafts", None)];
        assert_eq!(pick_folder(&folders, FolderRole::Drafts), None);
        assert_eq!(pick_folder(&[], FolderRole::Sent), None);
    }

    #[test]
    fn the_name_fallback_knows_the_names_real_servers_use() {
        // `Sent Messages` is Apple Mail's; any account it has ever touched has one. A server that
        // doesn't advertise SPECIAL-USE and calls it that must still get a copy of every message the
        // user sends — missing it means sent mail is saved NOWHERE, silently, which is the exact bug
        // this feature exists to fix.
        for name in [
            "Sent",
            "Sent Items",
            "Sent Mail",
            "Sent Messages",
            "INBOX.SentMail",
        ] {
            assert!(
                pick_folder(&[f(name, None)], FolderRole::Sent).is_some(),
                "{name} is a Sent folder"
            );
        }
        // The bin, under the names servers actually give it.
        for name in ["Trash", "Deleted", "Deleted Items", "Bin"] {
            assert!(
                pick_folder(&[f(name, None)], FolderRole::Trash).is_some(),
                "{name}"
            );
        }
        // …and the ones that only look like it. `Presentations` contains "sent"; the old substring
        // match filed sent mail there.
        assert_eq!(
            pick_folder(&[f("Presentations", None)], FolderRole::Sent),
            None
        );
        assert_eq!(pick_folder(&[f("Sent-2024", None)], FolderRole::Sent), None);
    }

    #[test]
    fn only_a_folder_called_drafts_is_the_drafts_folder() {
        // Singular `Draft` is somebody's own folder. The drafts folder is HIDDEN from the rail and has
        // its whole contents listed as deletable drafts — so claiming one on a guess makes a user's
        // mail unreachable and one click from gone. Only the server's flag may override the name.
        assert_eq!(pick_folder(&[f("Draft", None)], FolderRole::Drafts), None);
        assert_eq!(
            pick_folder(&[f("Draft", Some(FolderRole::Drafts))], FolderRole::Drafts),
            Some("Draft"),
            "…unless the server says so, in which case it is the drafts folder"
        );
        assert_eq!(
            pick_folder(&[f("INBOX.Drafts", None)], FolderRole::Drafts),
            Some("INBOX.Drafts")
        );
    }

    #[test]
    fn a_folder_with_several_special_uses_resolves_the_same_either_way_round() {
        // RFC 6154 lets a mailbox carry more than one special use (Dovecot's `special_use` takes a
        // list). Without a fixed priority, "where does sent mail go" would depend on nothing but the
        // order the server felt like sending its flags in.
        assert_eq!(
            pick_role(&[FolderRole::Sent, FolderRole::Archive]),
            pick_role(&[FolderRole::Archive, FolderRole::Sent])
        );
        assert_eq!(pick_role(&[]), None);
        assert_eq!(pick_role(&[FolderRole::Junk]), Some(FolderRole::Junk));
    }

    #[test]
    fn a_folder_the_app_files_mail_into_is_never_the_users_to_rename() {
        // `FolderRole::of` is the one answer to "is this the app's folder or the user's" — one list, so
        // it can't drift into a state where the app archives into a folder the rail lets you delete.
        assert_eq!(FolderRole::of("Archives", None), Some(FolderRole::Archive));
        assert_eq!(
            FolderRole::of("Sent Messages", None),
            Some(FolderRole::Sent)
        );
        assert_eq!(
            FolderRole::of("Papierkorb", Some("trash")),
            Some(FolderRole::Trash)
        );
        assert_eq!(FolderRole::of("Work", None), None);
        assert_eq!(FolderRole::of("Draft ideas", None), None);
    }

    #[test]
    fn a_roles_key_survives_the_round_trip_to_the_database() {
        for role in [
            FolderRole::Inbox,
            FolderRole::Drafts,
            FolderRole::Sent,
            FolderRole::Archive,
            FolderRole::Junk,
            FolderRole::Trash,
        ] {
            assert_eq!(FolderRole::from_key(role.key()), Some(role), "{role:?}");
        }
        // A key we don't know is not a role — never a panic, never a wrong role.
        assert_eq!(FolderRole::from_key("flagged"), None);
        assert_eq!(FolderRole::from_key(""), None);
        assert_eq!(FolderRole::from_key("Drafts"), None, "keys are lowercase");
    }

    #[test]
    fn the_name_fallback_matches_the_leaf_and_the_common_spellings() {
        assert!(FolderRole::Sent.matches_name("[Gmail]/Sent Mail"));
        assert!(FolderRole::Sent.matches_name("Sent Items"));
        assert!(FolderRole::Junk.matches_name("INBOX.Spam"));
        assert!(FolderRole::Trash.matches_name("Deleted"));
        assert!(FolderRole::Inbox.matches_name("inbox"));
        assert!(FolderRole::Archive.matches_name("INBOX.Archive"));
        // …and doesn't match things that only contain the word.
        assert!(!FolderRole::Sent.matches_name("Sent-2024"));
        assert!(!FolderRole::Drafts.matches_name("Draft ideas"));
        assert!(!FolderRole::Inbox.matches_name("INBOX.Work"));
    }
}

#[cfg(test)]
mod tests {
    use super::looks_like_email;

    #[test]
    fn accepts_plausible_address() {
        assert!(looks_like_email("user@example.com"));
    }

    #[test]
    fn rejects_without_at() {
        assert!(!looks_like_email("not-an-email"));
    }

    #[test]
    fn rejects_at_string_edges() {
        assert!(!looks_like_email("@example.com"));
        assert!(!looks_like_email("user@"));
    }

    #[test]
    fn rejects_addresses_with_spaces() {
        assert!(!looks_like_email("user name@example.com"));
    }
}
