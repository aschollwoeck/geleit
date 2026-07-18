//! `geleit-store` — the local SQLite store, the source of truth for the experience
//! (constitution P1). This crate owns the **account-scoped schema** and its migrations.
//!
//! **Encryption at rest** (SEC-1, ADR-0008): the app opens via [`Store::open_encrypted`] (SQLCipher
//! — `rusqlite`'s bundled-sqlcipher with vendored OpenSSL, so there's still no system dependency);
//! the key is applied with `PRAGMA key` at open and the whole DB is ciphertext on disk. `open` /
//! `open_in_memory` stay unencrypted for tests/dev. UI-agnostic (ADR-0003).

use rusqlite::{Connection, OptionalExtension};
use thiserror::Error;

/// Errors from the local store.
#[derive(Debug, Error)]
pub enum StoreError {
    /// An underlying SQLite error (wrapped — callers don't see `rusqlite` types directly).
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The email address failed basic validation. The address itself is deliberately **not**
    /// included — error messages must never carry addresses/PII (constitution P2, guidelines §4).
    #[error("invalid email address")]
    InvalidEmail,
    /// The database was created by a newer build than this one supports.
    #[error("database schema version {db} is newer than supported ({supported})")]
    SchemaTooNew { db: i64, supported: usize },
}

/// Ordered schema migrations, each applied once and tracked by SQLite's `user_version`.
/// **Append-only**: never edit a released migration — add a new entry to evolve the schema.
const MIGRATIONS: &[&str] = &[
    // 1 — initial account-scoped schema.
    "
    CREATE TABLE account (
        id           INTEGER PRIMARY KEY,
        email        TEXT NOT NULL UNIQUE,
        display_name TEXT,
        created_at   INTEGER NOT NULL
    );
    CREATE TABLE folder (
        id         INTEGER PRIMARY KEY,
        account_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
        name       TEXT NOT NULL,
        UNIQUE(account_id, name)
    );
    CREATE TABLE message (
        id              INTEGER PRIMARY KEY,
        account_id      INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
        folder_id       INTEGER NOT NULL REFERENCES folder(id)  ON DELETE CASCADE,
        uid             INTEGER,
        message_id      TEXT,
        subject         TEXT,
        from_name       TEXT,
        from_addr       TEXT,
        date            INTEGER,
        seen            INTEGER NOT NULL DEFAULT 0,
        flagged         INTEGER NOT NULL DEFAULT 0,
        has_attachments INTEGER NOT NULL DEFAULT 0,
        snippet         TEXT,
        UNIQUE(account_id, folder_id, uid)
    );
    CREATE TABLE body (
        message_id INTEGER PRIMARY KEY REFERENCES message(id) ON DELETE CASCADE,
        plain      TEXT,
        html       TEXT
    );
    CREATE INDEX message_folder_date ON message(folder_id, date DESC);
    ",
    // 2 — per-account IMAP connection settings (S1.10, manual account config). Nullable so a row
    // can exist before settings are known; `imap_allow_invalid` is a dev-only self-signed escape.
    "
    ALTER TABLE account ADD COLUMN imap_host TEXT;
    ALTER TABLE account ADD COLUMN imap_port INTEGER;
    ALTER TABLE account ADD COLUMN imap_username TEXT;
    ALTER TABLE account ADD COLUMN imap_allow_invalid INTEGER NOT NULL DEFAULT 0;
    ",
    // 3 — per-folder IMAP UIDVALIDITY (S2.3, incremental sync). NULL until first synced; a change
    // means the server's UIDs are no longer valid and the folder must be re-fetched.
    "
    ALTER TABLE folder ADD COLUMN uid_validity INTEGER;
    ",
    // 4 — attachment metadata (S3.5, view attachments). Bytes are not stored (yet); this is just
    // name/type/size so the reading pane can list what's attached.
    "
    CREATE TABLE attachment (
        id           INTEGER PRIMARY KEY,
        message_id   INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
        filename     TEXT,
        content_type TEXT NOT NULL,
        size_bytes   INTEGER NOT NULL
    );
    CREATE INDEX attachment_message ON attachment(message_id);
    ",
    // 5 — In-Reply-To header for conversation threading (S3.4); links a reply to its parent's
    // Message-ID.
    "
    ALTER TABLE message ADD COLUMN in_reply_to TEXT;
    ",
    // 6 — per-account SMTP settings (M4, sending). Nullable until configured. Username + password +
    // the self-signed escape are shared with IMAP; only host/port/security differ. `smtp_security`
    // is 'implicit' or 'starttls'.
    "
    ALTER TABLE account ADD COLUMN smtp_host TEXT;
    ALTER TABLE account ADD COLUMN smtp_port INTEGER;
    ALTER TABLE account ADD COLUMN smtp_security TEXT;
    ",
    // 7 — per-account signature (SEND-7/ACC-7), auto-appended to composed messages. NULL = none.
    "
    ALTER TABLE account ADD COLUMN signature TEXT;
    ",
    // 8 — original To/Cc recipients (comma-joined bare addresses) for reply-all (SEND-2). NULL on
    // messages synced before this migration; reply-all falls back to reply-to-sender for those.
    "
    ALTER TABLE message ADD COLUMN to_addrs TEXT;
    ALTER TABLE message ADD COLUMN cc_addrs TEXT;
    ",
    // 9 — local drafts (SEND-5): unsent messages saved on this device. `reference_ids` is the
    // comma-joined References chain (`references` is a SQL keyword).
    "
    CREATE TABLE draft (
        id            INTEGER PRIMARY KEY,
        account_id    INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
        to_addrs      TEXT NOT NULL DEFAULT '',
        cc_addrs      TEXT NOT NULL DEFAULT '',
        subject       TEXT NOT NULL DEFAULT '',
        body          TEXT NOT NULL DEFAULT '',
        in_reply_to   TEXT,
        reference_ids TEXT,
        updated_at    INTEGER NOT NULL
    );
    CREATE INDEX draft_account_updated ON draft(account_id, updated_at DESC);
    ",
    // 10 — full-text search index (SEARCH-1/2/3). An FTS5 table keyed by message id (rowid) over
    // subject / sender / body. It lives INSIDE the SQLCipher database, so the index is encrypted at
    // rest like everything else (ADR-0010 — chosen over an external engine like tantivy, whose files
    // would be plaintext on disk). Rows are filled by `index_message`; the trigger removes them when
    // a message is deleted — including FK cascades from folder/account deletes, which fire it too.
    "
    CREATE VIRTUAL TABLE message_fts USING fts5(
        subject, sender, body,
        tokenize = 'unicode61 remove_diacritics 2'
    );
    CREATE TRIGGER message_fts_ad AFTER DELETE ON message BEGIN
        DELETE FROM message_fts WHERE rowid = old.id;
    END;
    ",
    // 11 — attachments saved with a draft (SEND-4/5). Unlike `attachment` (received-message
    // metadata, no bytes), these carry the file `data` so a resumed draft keeps its files. Encrypted
    // at rest with the rest of the DB. Cascade-deleted with the draft.
    "
    CREATE TABLE draft_attachment (
        id           INTEGER PRIMARY KEY,
        draft_id     INTEGER NOT NULL REFERENCES draft(id) ON DELETE CASCADE,
        filename     TEXT,
        content_type TEXT NOT NULL,
        data         BLOB NOT NULL
    );
    CREATE INDEX draft_attachment_draft ON draft_attachment(draft_id);
    ",
    // 12 — app-wide settings (APP-4): a simple key/value table (e.g. theme = light/dark). Not
    // account-scoped — these are device/app preferences.
    "
    CREATE TABLE setting (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    ",
    // 13 — which server folder holds a copy of this draft, when the opt-in "sync drafts" setting is
    // on (SEND-5). NULL = local-only (the default). Only the folder is stored: the copy itself is
    // identified by the stable Message-ID we stamp on it, so a re-save/send/discard finds and
    // expunges it by search — no UID to go stale when a mailbox's UIDVALIDITY resets.
    "
    ALTER TABLE draft ADD COLUMN server_folder TEXT;
    ",
    // 14 — has this folder ever completed a sync? (NOTIF-1.) Until it has, "not in our store" means
    // "we have never looked", not "new mail" — so the first sync of a folder must announce nothing,
    // or a new account would fire a notification per message in its inbox. Set only after a sync
    // *finishes*, so a sync that dies half-way doesn't leave the folder falsely primed; cleared again
    // if the server resets UIDVALIDITY (which makes every message look new once more).
    "
    ALTER TABLE folder ADD COLUMN primed INTEGER NOT NULL DEFAULT 0;
    ",
    // 15 — a draft's Message-ID is now a **stored fact**, not a function of its id.
    //
    // It used to be derived: `<geleit-draft-{account}-{draft}@geleit.local>`. But `draft.id` is a bare
    // SQLite rowid, and SQLite **reuses** the id of the highest deleted row — so a new, unrelated draft
    // could inherit a deleted one's identity. Two things then went wrong, both silent:
    //
    //   * a copy of the deleted draft still on the server (the expunge failed — offline) folded into
    //     the new draft in the Drafts list and vanished from view for good; and, far worse,
    //   * the next save of the new draft expunged **by Message-ID**, destroying that stranded draft's
    //     content on the server.
    //
    // So each draft now carries its own id with a random suffix, minted once and never reused. Existing
    // rows are backfilled with the derived form they already have copies under, so those copies stay
    // findable (and expungeable) exactly as before.
    "
    ALTER TABLE draft ADD COLUMN msgid TEXT NOT NULL DEFAULT '';
    UPDATE draft SET msgid = '<geleit-draft-' || account_id || '-' || id || '@geleit.local>'
      WHERE msgid = '';
    ",
    // 16 — what a folder is *for*, as the server itself says (RFC 6154 SPECIAL-USE: `\Drafts`,
    // `\Sent`, `\Trash`, `\Archive`, `\Junk` on the LIST response).
    //
    // Until now every special folder was found by matching the English word — so on a provider that
    // localizes them (GMX's `Entwürfe`, `Gesendet`, `Papierkorb`) GeleitMail found *none* of them: sent
    // mail was saved nowhere, Archive and Junk declined to work, and drafts never merged. NULL = the
    // server didn't say (or we haven't re-listed since this landed), and the name match still applies.
    "
    ALTER TABLE folder ADD COLUMN role TEXT;
    ",
    // 17 — has the user been *told* about this message? (NOTIF-1.)
    //
    // Whether mail is news was, until now, a property of one sync's diff against the store: whoever
    // wrote the message first ate the signal. So a message that arrived while the **backfill** thread
    // was running got stored by the backfill, was therefore no longer "absent from our store", and no
    // later sync would ever call it new — it sat in the inbox, unread and unannounced, forever.
    //
    // Being told is now a durable fact about the message, not a property of a diff. `0` = we owe the
    // user a notification; anything already in a folder at its first sync, and anything the backfill
    // fetches from *below* the newest UID we already had (i.e. old mail, which is what backfill is
    // for), is written as `1` — accounted for, never announced.
    "
    ALTER TABLE message ADD COLUMN notified INTEGER NOT NULL DEFAULT 1;
    ",
    // 18 — has a local read/star change been confirmed on the server yet? (SYNC-5.)
    //
    // A sync now PULLS the server's `\Seen`/`\Flagged` for mail we already hold, so "read on another
    // device" reaches this one. But that same pull must not UNDO a change the user just made here whose
    // write-back to the server hasn't landed (or failed): reading a message marks it read locally and
    // writes `\Seen` back on a worker — if a sync runs before that lands, the server still says unread,
    // and a naive server-wins pull would flip the user's read straight back off.
    //
    // `1` = this message has a local flag change the server hasn't confirmed → the pull leaves it
    // alone. Set when a local change is made, cleared when its write-back succeeds. A write-back that
    // never succeeds keeps the flag dirty forever, so local intent wins — which is exactly the old
    // behaviour, preserved.
    "
    ALTER TABLE message ADD COLUMN flags_dirty INTEGER NOT NULL DEFAULT 0;
    ",
    // 19 — the outbox (SEND-10): messages composed but not yet delivered, so a send survives being
    // offline. A send that can't reach the SMTP server (the usual offline case) is queued here as the
    // **already-built RFC 5322 bytes** plus the envelope needed to hand them to a server later, and the
    // scheduler retries it every sweep until it goes out. `failed` marks one the server *rejected* (a
    // permanent 5xx — a bad address, say): retrying that never helps, so it stops and is surfaced to
    // the user instead of looping or vanishing. `subject` is kept only to show the user what's waiting.
    "
    CREATE TABLE outbox (
        id           INTEGER PRIMARY KEY,
        account_id   INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
        mail_from    TEXT NOT NULL,
        recipients   TEXT NOT NULL,
        subject      TEXT NOT NULL DEFAULT '',
        raw          BLOB NOT NULL,
        failed       INTEGER NOT NULL DEFAULT 0,
        last_error   TEXT,
        -- If this message was a resumed copy of a draft on the provider (opt-in 'sync drafts'), the
        -- folder + stable Message-ID of that copy, so it can be expunged once the message actually
        -- goes out. Otherwise NULL. Without this, a draft sent via the outbox stays in the provider's
        -- Drafts folder and reappears as a draft.
        draft_folder TEXT,
        draft_msgid  TEXT,
        created_at   INTEGER NOT NULL
    );
    CREATE INDEX outbox_account ON outbox(account_id);
    ",
    // 20 — snooze (ORG-9): hide a message until a time you pick, then let it come back. A **local**
    // property — nothing moves on the server — so it can't fail offline and needs no live server to be
    // correct. NULL = not snoozed; a unix timestamp = hidden from its folder list and left out of the
    // unread badge until then, when the scheduler resurfaces it (clears this + re-owes a notification).
    "
    ALTER TABLE message ADD COLUMN snoozed_until INTEGER;
    CREATE INDEX message_snoozed ON message(snoozed_until) WHERE snoozed_until IS NOT NULL;
    ",
    // 21 — client-side rules / filters (ORG-8). A rule is one condition (a field contains some text)
    // and one or more actions (move to a folder, mark read, star), applied to new INBOX mail as it
    // syncs. `message.filtered` is the durable 'has a rule pass happened for this message yet?' marker,
    // the exact shape of `notified` (migration 17): new mail arrives 0 (owed a pass), everything already
    // here at upgrade is 1 (rules are never applied retroactively — that's opt-in 'Run on inbox now').
    "
    CREATE TABLE rule (
        id            INTEGER PRIMARY KEY,
        account_id    INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
        field         TEXT NOT NULL,          -- 'from' | 'subject' | 'to'
        pattern       TEXT NOT NULL,          -- the case-insensitive substring to look for
        target_folder TEXT,                   -- move here when matched (NULL = don't move)
        mark_read     INTEGER NOT NULL DEFAULT 0,
        star          INTEGER NOT NULL DEFAULT 0,
        position      INTEGER NOT NULL,       -- evaluation order; first match wins
        created_at    INTEGER NOT NULL
    );
    CREATE INDEX rule_account ON rule(account_id, position);
    ALTER TABLE message ADD COLUMN filtered INTEGER NOT NULL DEFAULT 1;
    ",
    // 22 — offline moves / deletes (OFF-4): a move (Archive / Trash / Spam / Move to…) you make while
    // offline must survive and reach the server on reconnect, the way the outbox (19) does for sends and
    // `flags_dirty` (18) does for read/star. NULL = normal; a folder **name** = this message is queued to
    // move there. Two facts in one column, exactly like `flags_dirty`: it hides the row from every listing
    // (the move looks instantaneous) AND, with the row's own source folder + uid, is the durable queue the
    // flush drains. Deliberately absent from `upsert_message`'s ON CONFLICT set, so a re-sync of the source
    // folder — where the message still is until the move lands — preserves the marker rather than
    // re-showing the message. The mail is never expunged, so a move that never reaches the server loses
    // nothing; it stays safe in its source folder, hidden, until the flush finally pushes it.
    "
    ALTER TABLE message ADD COLUMN pending_move TEXT;
    CREATE INDEX message_pending_move ON message(account_id) WHERE pending_move IS NOT NULL;
    ",
];

/// A parsed search query (SEARCH-1/4): an FTS5 `MATCH` string (`None` when there are no full-text
/// terms) plus structured filters that aren't full-text.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedSearch {
    pub match_query: Option<String>,
    pub require_attachment: bool,
}

/// Parse free user input into an FTS5 `MATCH` plus filters, supporting simple operators (SEARCH-4):
/// `from:TEXT` and `subject:TEXT` scope a term to a column; `has:attachment` filters to messages with
/// attachments. Bare words search all columns. Every term is **quoted** so punctuation can't inject
/// FTS5 operators (`*`, `OR`, `NEAR`, stray quotes); the final full-text term gets a trailing `*` for
/// prefix matching (type-ahead).
pub fn parse_search(input: &str) -> ParsedSearch {
    let mut require_attachment = false;
    let mut terms: Vec<String> = Vec::new();
    for tok in input.split_whitespace() {
        if tok.eq_ignore_ascii_case("has:attachment") || tok.eq_ignore_ascii_case("has:attachments")
        {
            require_attachment = true;
        } else if let Some(rest) = strip_prefix_ci(tok, "from:") {
            if let Some(p) = fts_phrase(rest) {
                terms.push(format!("sender:{p}"));
            }
        } else if let Some(rest) = strip_prefix_ci(tok, "subject:") {
            if let Some(p) = fts_phrase(rest) {
                terms.push(format!("subject:{p}"));
            }
        } else if let Some(p) = fts_phrase(tok) {
            terms.push(p);
        }
    }
    let match_query = (!terms.is_empty()).then(|| format!("{}*", terms.join(" ")));
    ParsedSearch {
        match_query,
        require_attachment,
    }
}

/// The local folder that holds opened `.eml` files (READ-10). It never exists on the IMAP server,
/// so folder pruning keeps it (see [`Store::prune_folders`]).
pub const SAVED_FOLDER: &str = "Saved";

/// Whether `name` is a local-only folder — kept across server folder syncs, never pushed to the server.
fn is_local_folder(name: &str) -> bool {
    name.eq_ignore_ascii_case(SAVED_FOLDER)
}

/// Sort rank for a folder name (lower = earlier). Inbox first, then the common special folders in a
/// conventional order, then everything else (same rank → ordered by name). Matches provider variants
/// loosely (e.g. "Deleted Items" → trash, "Junk Email" → junk).
fn folder_rank(name: &str, role: Option<&str>) -> u8 {
    // The server's own word first, so a provider that calls its bin `Papierkorb` still gets it sorted
    // with the special folders rather than filed under P with the user's own.
    if let Some(role) = role.and_then(geleit_core::FolderRole::from_key) {
        return match role {
            geleit_core::FolderRole::Inbox => 0,
            geleit_core::FolderRole::Drafts => 1,
            geleit_core::FolderRole::Sent => 2,
            geleit_core::FolderRole::Archive => 3,
            geleit_core::FolderRole::Junk => 4,
            geleit_core::FolderRole::Trash => 5,
        };
    }
    let n = name.to_lowercase();
    match n.as_str() {
        "inbox" => 0,
        "drafts" => 1,
        "sent" | "sent mail" | "sent items" => 2,
        "archive" | "all mail" => 3,
        _ if n.contains("junk") || n.contains("spam") => 4,
        _ if n.contains("trash") || n.contains("deleted") || n.contains("bin") => 5,
        _ => 6,
    }
}

/// Quote a token as an FTS5 phrase, or `None` if it carries no searchable (alphanumeric) content.
fn fts_phrase(tok: &str) -> Option<String> {
    tok.chars()
        .any(char::is_alphanumeric)
        .then(|| format!("\"{}\"", tok.replace('"', "\"\"")))
}

/// Case-insensitive `strip_prefix` for ASCII operator prefixes; char-boundary safe.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Map a 12-column header SELECT (id, uid, message_id, in_reply_to, subject, from_name, from_addr,
/// date, seen, has_attachments, snippet, flagged) to a [`MessageHeader`]. `to_addrs`/`cc_addrs` are
/// left `None` — listings/search don't need them ([`Store::header_by_id`] reads them).
fn header_from_row(r: &rusqlite::Row) -> rusqlite::Result<MessageHeader> {
    Ok(MessageHeader {
        id: r.get(0)?,
        uid: r.get(1)?,
        message_id: r.get(2)?,
        in_reply_to: r.get(3)?,
        subject: r.get(4)?,
        from_name: r.get(5)?,
        from_addr: r.get(6)?,
        to_addrs: None,
        cc_addrs: None,
        date: r.get(7)?,
        seen: r.get(8)?,
        has_attachments: r.get(9)?,
        snippet: r.get(10)?,
        flagged: r.get(11)?,
    })
}

/// An account row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: i64,
    pub email: String,
    pub display_name: Option<String>,
}

/// Per-account IMAP connection settings. `allow_invalid_certs` is a dev-only escape for self-signed
/// servers (never set for real accounts); the password lives in the secret store, not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapSettings {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub allow_invalid_certs: bool,
}

/// Transport security for SMTP (the username/password/self-signed flag are shared with IMAP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpSecurityKind {
    /// Implicit TLS (typically port 465).
    Implicit,
    /// STARTTLS upgrade (typically port 587).
    StartTls,
}

impl SmtpSecurityKind {
    fn as_str(self) -> &'static str {
        match self {
            SmtpSecurityKind::Implicit => "implicit",
            SmtpSecurityKind::StartTls => "starttls",
        }
    }
    fn from_str(s: &str) -> Self {
        // default to the safer implicit-TLS reading of any unrecognised value
        match s {
            "starttls" => SmtpSecurityKind::StartTls,
            _ => SmtpSecurityKind::Implicit,
        }
    }
}

/// Per-account SMTP connection settings (host/port/security). Username, password, and the
/// self-signed escape are shared with [`ImapSettings`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub security: SmtpSecurityKind,
}

/// The editable content of a draft message (SEND-5). Addresses are comma-joined, as typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DraftContent {
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

/// A stored draft: its id, content, and last-saved time (unix seconds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftRow {
    pub id: i64,
    /// This draft's own RFC 5322 `Message-ID`, minted when it was first saved and never reused — it is
    /// how a copy on the server is recognised as *this* draft's. Deliberately **not** derived from
    /// `id`: SQLite reuses the ids of deleted rows (migration 15).
    pub msgid: String,
    pub content: DraftContent,
    pub updated_at: i64,
    /// The server folder holding a copy of this draft, when the opt-in "sync drafts" setting put one
    /// there. `None` = local-only (the default). The copy itself is found by its Message-ID.
    pub server_folder: Option<String>,
}

/// An attachment saved with a draft (SEND-4/5): the file bytes plus its name/type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftAttachment {
    pub filename: Option<String>,
    pub content_type: String,
    pub data: Vec<u8>,
}

/// A folder/mailbox row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub id: i64,
    pub account_id: i64,
    pub name: String,
    /// What the folder is **for**, as the server declared it (RFC 6154 SPECIAL-USE): `drafts`, `sent`,
    /// `trash`, `archive`, `junk`, `inbox`. `None` = the server didn't say, and the folder's *name* is
    /// all we have to go on. See `geleit_core::FolderRole`.
    pub role: Option<String>,
}

/// A message envelope to insert/update. `date` is unix seconds; `uid` is the IMAP UID.
#[derive(Debug, Clone, Default)]
pub struct NewMessage {
    pub uid: Option<i64>,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_addr: Option<String>,
    /// Comma-joined bare To/Cc addresses (for reply-all). Set on envelope sync.
    pub to_addrs: Option<String>,
    pub cc_addrs: Option<String>,
    pub date: Option<i64>,
    pub seen: bool,
    /// Starred/`\Flagged` on the server at first sync (ORG-4). Local stars are preserved on re-sync.
    pub flagged: bool,
    pub has_attachments: bool,
    pub snippet: Option<String>,
    /// Do we owe the user a notification for this message (NOTIF-1)?
    ///
    /// **Deliberately phrased so that `Default` (`false`) means "owes nobody".** The column is
    /// `notified`, whose default is the same thing said the other way round — and a writer that reaches
    /// for `..Default::default()` (the `.eml` importer does) must never accidentally mint a debt and
    /// pop up a notification for a file the user opened by hand. Only ever written on insert: a re-sync
    /// of a message must not re-announce it.
    pub owed_notification: bool,
}

/// One snoozed message as the Snoozed view shows it (ORG-9) — who it's from, its subject, and when it
/// comes back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnoozedItem {
    pub id: i64,
    pub from_name: Option<String>,
    pub from_addr: Option<String>,
    pub subject: Option<String>,
    /// Unix timestamp it resurfaces at.
    pub snoozed_until: i64,
}

/// A client-side mail rule (ORG-8): one condition, one or more actions. `field` is a
/// `geleit_core::rule::RuleField` key (`from`/`subject`/`to`); the app matches with `core::rule`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: i64,
    pub account_id: i64,
    pub field: String,
    pub pattern: String,
    /// Folder name to move a matched message into, or `None` to leave it where it is.
    pub target_folder: Option<String>,
    pub mark_read: bool,
    pub star: bool,
}

/// One INBOX message awaiting a rule pass (ORG-8) — just the fields a condition can test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnfilteredMessage {
    pub id: i64,
    pub from_name: Option<String>,
    pub from_addr: Option<String>,
    pub subject: Option<String>,
    pub to_addrs: Option<String>,
}

/// One queued move waiting to reach the server (OFF-4) — everything `run_move` needs to push it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingMove {
    /// The local row to delete once the server confirms the move.
    pub id: i64,
    /// The folder the message is still in on the server (the move's source).
    pub source_folder: String,
    /// Its IMAP uid in that folder.
    pub uid: i64,
    /// The folder to move it to.
    pub target_folder: String,
}

/// One outbox message as the user sees it in the outbox view (SEND-10) — no raw bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxItem {
    pub id: i64,
    pub recipients: Vec<String>,
    pub subject: String,
    /// The server rejected it (a permanent error); it's no longer retried and shows its `last_error`.
    pub failed: bool,
    pub last_error: Option<String>,
}

/// A message waiting in the outbox to be delivered (SEND-10): the built RFC 5322 bytes plus the
/// envelope needed to hand them to an SMTP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxMessage {
    pub id: i64,
    pub mail_from: String,
    pub recipients: Vec<String>,
    pub subject: String,
    pub raw: Vec<u8>,
    /// The provider's Drafts copy to expunge once this goes out (opt-in 'sync drafts'), or `None`.
    pub server_draft: Option<(String, String)>,
}

/// A read/star change this account owes the server (SYNC-5) — a row in the durable write-back queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingFlagWriteback {
    pub id: i64,
    pub folder: String,
    pub uid: i64,
    pub seen: bool,
    pub flagged: bool,
}

/// A message the user has not been told about yet — what a notification needs, and nothing more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingNotification {
    pub id: i64,
    pub from_name: Option<String>,
    pub from_addr: Option<String>,
    pub subject: Option<String>,
}

/// A draft as it sits in the provider's Drafts folder — what the Drafts list needs to show it, decide
/// whether it's one of ours, and warn before a plain-text composer eats its formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderDraft {
    pub id: i64,
    /// The RFC 5322 `Message-ID` — how a copy GeleitMail itself uploaded is recognised.
    pub message_id: Option<String>,
    pub to_addrs: Option<String>,
    pub cc_addrs: Option<String>,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    /// The `Date:` header — when the draft was written, as the provider recorded it.
    pub date: Option<i64>,
    /// It has an HTML body: continuing it in a plain-text composer keeps the words, not the styling.
    pub formatted: bool,
}

/// A message header as read back for listing (newest-first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageHeader {
    pub id: i64,
    pub uid: Option<i64>,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_addr: Option<String>,
    /// Comma-joined bare To/Cc addresses (for reply-all). Only populated by [`Store::header_by_id`];
    /// the folder listing leaves these `None` (it doesn't need them).
    pub to_addrs: Option<String>,
    pub cc_addrs: Option<String>,
    pub date: Option<i64>,
    pub seen: bool,
    pub flagged: bool,
    pub has_attachments: bool,
    pub snippet: Option<String>,
}

/// A stored message body (plaintext and/or HTML).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredBody {
    pub plain: Option<String>,
    pub html: Option<String>,
}

/// Attachment metadata (name/type/size) — used both to store and to read back (S3.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub filename: Option<String>,
    pub content_type: String,
    pub size: i64,
}

/// The local store (one SQLite connection).
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (or create) an **unencrypted** store at `path` (tests / dev). The app uses
    /// [`open_encrypted`](Self::open_encrypted).
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self, StoreError> {
        Self::init(Connection::open(path)?)
    }

    /// Open (or create) an **encrypted** store at `path`, unlocked with `key` (SQLCipher; SEC-1,
    /// ADR-0008). `key` is raw bytes (32 expected). A wrong key surfaces as an error on first read.
    pub fn open_encrypted<P: AsRef<std::path::Path>>(
        path: P,
        key: &[u8],
    ) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        // SQLCipher: the key must be set before any other operation. Use the raw-key form
        // (`x'..'`) so the bytes are the key directly, not run through key derivation.
        // The hex is always valid SQL (64 chars of [0-9a-f]), so this PRAGMA can't fail at
        // prepare-time and surface the key inside a `SqlInputError`; a wrong key fails later on the
        // first read (in `migrate`), whose SQL carries no key (P2).
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        conn.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))?;
        Self::init(conn)
    }

    /// Open an in-memory store (tests / ephemeral use). Unencrypted.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        // `foreign_keys` — SQLite defaults it OFF, and the schema leans on ON DELETE CASCADE.
        //
        // `journal_mode = WAL` + `busy_timeout` — the app runs **more than one connection to this
        // file**: the IPC commands hold one, and the engine's workers (refresh, backfill, the
        // background scheduler) each open their own via `open_store`. Under the default rollback
        // journal a reader and a writer collide, and with `busy_timeout = 0` the loser fails
        // *immediately* with SQLITE_BUSY rather than waiting its turn. WAL lets readers proceed while
        // one writer works; the timeout makes a second writer queue instead of erroring. Without both,
        // a background sync landing while the user scrolls is a coin-flip failure.
        //
        // WAL is persisted in the database file, so this is a no-op on later opens. It is skipped for
        // `:memory:` (which has no WAL) — `execute_batch` would just report the mode back, but being
        // explicit keeps the in-memory test path identical to production apart from the journal.
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA busy_timeout = 5000;",
        )?;
        let mut store = Self { conn };
        store.migrate()?;
        store.backfill_search_index()?;
        Ok(store)
    }

    /// One-time build of the FTS index for messages that predate migration #10 (SEARCH backfill).
    /// Runs only when the index is empty; once messages are indexed it's skipped on later opens.
    /// (No separate message-count guard: reindexing zero messages is a harmless no-op.)
    fn backfill_search_index(&self) -> Result<(), StoreError> {
        let indexed: i64 = self
            .conn
            .query_row("SELECT count(*) FROM message_fts", [], |r| r.get(0))?;
        if indexed == 0 {
            self.reindex_all()?;
        }
        Ok(())
    }

    /// Apply any migrations newer than the database's current `user_version`, each in its own
    /// transaction. Idempotent: already-applied migrations are skipped.
    fn migrate(&mut self) -> Result<(), StoreError> {
        let current: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        // Guard against a database written by a newer build (more migrations than we know):
        // running against an unknown schema would be worse than refusing to open.
        let applied = usize::try_from(current).unwrap_or(usize::MAX);
        if applied > MIGRATIONS.len() {
            return Err(StoreError::SchemaTooNew {
                db: current,
                supported: MIGRATIONS.len(),
            });
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            // `user_version` cannot be parameterized; the value is a trusted migration index.
            tx.execute_batch(&format!("PRAGMA user_version = {};", i + 1))?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Add an account (email validated via `geleit-core`); returns its id.
    pub fn add_account(&self, email: &str, display_name: Option<&str>) -> Result<i64, StoreError> {
        if !geleit_core::looks_like_email(email) {
            return Err(StoreError::InvalidEmail);
        }
        self.conn.execute(
            "INSERT INTO account (email, display_name, created_at) \
             VALUES (?1, ?2, strftime('%s', 'now'))",
            (email, display_name),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Add an account together with its IMAP settings; returns its id.
    pub fn add_imap_account(
        &self,
        email: &str,
        display_name: Option<&str>,
        imap: &ImapSettings,
    ) -> Result<i64, StoreError> {
        if !geleit_core::looks_like_email(email) {
            return Err(StoreError::InvalidEmail);
        }
        self.conn.execute(
            "INSERT INTO account \
             (email, display_name, created_at, imap_host, imap_port, imap_username, imap_allow_invalid) \
             VALUES (?1, ?2, strftime('%s', 'now'), ?3, ?4, ?5, ?6)",
            (
                email,
                display_name,
                &imap.host,
                imap.port,
                &imap.username,
                imap.allow_invalid_certs,
            ),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Replace an account's IMAP settings (used for reconnect / re-config).
    pub fn update_imap_settings(
        &self,
        account_id: i64,
        imap: &ImapSettings,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE account SET imap_host = ?2, imap_port = ?3, imap_username = ?4, \
             imap_allow_invalid = ?5 WHERE id = ?1",
            (
                account_id,
                &imap.host,
                imap.port,
                &imap.username,
                imap.allow_invalid_certs,
            ),
        )?;
        Ok(())
    }

    /// An account's IMAP settings, or `None` if not configured (host unset).
    pub fn imap_settings(&self, account_id: i64) -> Result<Option<ImapSettings>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT imap_host, imap_port, imap_username, imap_allow_invalid \
                 FROM account WHERE id = ?1",
                [account_id],
                |r| {
                    let host: Option<String> = r.get(0)?;
                    let port: Option<i64> = r.get(1)?;
                    let username: Option<String> = r.get(2)?;
                    let allow_invalid: bool = r.get(3)?;
                    Ok(host
                        .zip(port)
                        .zip(username)
                        .map(|((host, port), username)| ImapSettings {
                            host,
                            port: port as u16,
                            username,
                            allow_invalid_certs: allow_invalid,
                        }))
                },
            )
            .optional()?
            .flatten())
    }

    /// Replace an account's SMTP settings (for sending, M4).
    pub fn update_smtp_settings(
        &self,
        account_id: i64,
        smtp: &SmtpConfig,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE account SET smtp_host = ?2, smtp_port = ?3, smtp_security = ?4 WHERE id = ?1",
            (account_id, &smtp.host, smtp.port, smtp.security.as_str()),
        )?;
        Ok(())
    }

    /// An account's SMTP settings, or `None` if not configured (host unset).
    pub fn smtp_settings(&self, account_id: i64) -> Result<Option<SmtpConfig>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT smtp_host, smtp_port, smtp_security FROM account WHERE id = ?1",
                [account_id],
                |r| {
                    let host: Option<String> = r.get(0)?;
                    let port: Option<i64> = r.get(1)?;
                    let security: Option<String> = r.get(2)?;
                    Ok(host.zip(port).map(|(host, port)| SmtpConfig {
                        host,
                        port: port as u16,
                        security: security
                            .as_deref()
                            .map(SmtpSecurityKind::from_str)
                            .unwrap_or(SmtpSecurityKind::Implicit),
                    }))
                },
            )
            .optional()?
            .flatten())
    }

    /// Set an account's signature (SEND-7). An empty string clears it (stored as NULL).
    pub fn update_signature(&self, account_id: i64, signature: &str) -> Result<(), StoreError> {
        let value: Option<&str> = (!signature.is_empty()).then_some(signature);
        self.conn.execute(
            "UPDATE account SET signature = ?2 WHERE id = ?1",
            (account_id, value),
        )?;
        Ok(())
    }

    /// An account's signature, or `None` if unset.
    pub fn signature(&self, account_id: i64) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT signature FROM account WHERE id = ?1",
                [account_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    /// Save a draft (SEND-5): update `id` if given, else insert. Returns the draft's id.
    pub fn save_draft(
        &self,
        account_id: i64,
        id: Option<i64>,
        c: &DraftContent,
    ) -> Result<i64, StoreError> {
        let refs: Option<String> = (!c.references.is_empty()).then(|| c.references.join(","));
        match id {
            Some(id) => {
                self.conn.execute(
                    "UPDATE draft SET to_addrs = ?2, cc_addrs = ?3, subject = ?4, body = ?5, \
                     in_reply_to = ?6, reference_ids = ?7, updated_at = strftime('%s','now') \
                     WHERE id = ?1",
                    (id, &c.to, &c.cc, &c.subject, &c.body, &c.in_reply_to, &refs),
                )?;
                Ok(id)
            }
            None => {
                self.conn.execute(
                    "INSERT INTO draft \
                     (account_id, to_addrs, cc_addrs, subject, body, in_reply_to, reference_ids, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, strftime('%s','now'))",
                    (account_id, &c.to, &c.cc, &c.subject, &c.body, &c.in_reply_to, &refs),
                )?;
                let id = self.conn.last_insert_rowid();
                // The random suffix is the whole point: the row id alone is reused by SQLite after a
                // delete, and a draft that inherits a dead draft's Message-ID expunges *its* copy off
                // the server. `randomblob` keeps this in SQL — no RNG dependency for a mail store.
                self.conn.execute(
                    "UPDATE draft SET msgid = '<geleit-draft-' || account_id || '-' || id || '-' || \
                     lower(hex(randomblob(6))) || '@geleit.local>' WHERE id = ?1",
                    [id],
                )?;
                Ok(id)
            }
        }
    }

    /// All drafts for an account, newest-saved first.
    pub fn list_drafts(&self, account_id: i64) -> Result<Vec<DraftRow>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, to_addrs, cc_addrs, subject, body, in_reply_to, reference_ids, updated_at, \
             server_folder, msgid \
             FROM draft WHERE account_id = ?1 ORDER BY updated_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([account_id], Self::draft_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// A single draft by id, or `None`.
    pub fn draft_by_id(&self, id: i64) -> Result<Option<DraftRow>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, to_addrs, cc_addrs, subject, body, in_reply_to, reference_ids, \
                 updated_at, server_folder, msgid FROM draft WHERE id = ?1",
                [id],
                Self::draft_from_row,
            )
            .optional()?)
    }

    /// Delete a draft (e.g. after it's sent, or discarded). Idempotent. Attachments cascade.
    pub fn delete_draft(&self, id: i64) -> Result<(), StoreError> {
        self.conn.execute("DELETE FROM draft WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Replace all of a draft's saved attachments with `atts` (SEND-4/5). Pass an empty slice to
    /// clear them. Done in one transaction so a resumed draft never sees a half-updated set.
    pub fn replace_draft_attachments(
        &self,
        draft_id: i64,
        atts: &[DraftAttachment],
    ) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM draft_attachment WHERE draft_id = ?1",
            [draft_id],
        )?;
        for a in atts {
            tx.execute(
                "INSERT INTO draft_attachment (draft_id, filename, content_type, data) \
                 VALUES (?1, ?2, ?3, ?4)",
                (draft_id, &a.filename, &a.content_type, &a.data),
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// A draft's saved attachments, in insertion order.
    pub fn draft_attachments(&self, draft_id: i64) -> Result<Vec<DraftAttachment>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT filename, content_type, data FROM draft_attachment \
             WHERE draft_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([draft_id], |r| {
            Ok(DraftAttachment {
                filename: r.get(0)?,
                content_type: r.get(1)?,
                data: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn draft_from_row(r: &rusqlite::Row) -> rusqlite::Result<DraftRow> {
        let reference_ids: Option<String> = r.get(6)?;
        Ok(DraftRow {
            id: r.get(0)?,
            content: DraftContent {
                to: r.get(1)?,
                cc: r.get(2)?,
                subject: r.get(3)?,
                body: r.get(4)?,
                in_reply_to: r.get(5)?,
                references: reference_ids
                    .filter(|s| !s.is_empty())
                    .map(|s| s.split(',').map(str::to_owned).collect())
                    .unwrap_or_default(),
            },
            updated_at: r.get(7)?,
            server_folder: r.get(8)?,
            msgid: r.get(9)?,
        })
    }

    /// The account a draft belongs to, or `None` if it's gone. Needed to reach the right server.
    pub fn account_for_draft(&self, draft_id: i64) -> Result<Option<i64>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT account_id FROM draft WHERE id = ?1",
                [draft_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Every draft of `account_id` that has a copy on the server, as `(draft_id, folder)` — for
    /// sweeping those copies away when the "sync drafts" setting is turned off (SEND-5).
    pub fn drafts_with_server_copies(
        &self,
        account_id: i64,
    ) -> Result<Vec<(i64, String, String)>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, server_folder, msgid FROM draft \
             WHERE account_id = ?1 AND server_folder IS NOT NULL",
        )?;
        let rows = stmt.query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Record (or clear, with `None`) the server folder holding this draft's copy (SEND-5).
    pub fn set_draft_server_folder(
        &self,
        draft_id: i64,
        folder: Option<&str>,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE draft SET server_folder = ?2 WHERE id = ?1",
            (draft_id, folder),
        )?;
        Ok(())
    }

    /// Read an app-wide setting (APP-4), or `None` if unset.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// Write an app-wide setting (APP-4), replacing any previous value.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO setting (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (key, value),
        )?;
        Ok(())
    }

    /// Delete an account and everything under it (folders/messages/bodies cascade).
    pub fn delete_account(&self, account_id: i64) -> Result<(), StoreError> {
        // The account's per-account settings live in a k/v table keyed by its **id**, and they do not
        // cascade. SQLite reuses the row ids of deleted rows, so leaving them behind means the *next*
        // account added inherits them: add an account, and it silently has notifications switched off,
        // with no way to find out why. Take them with the account.
        self.conn.execute(
            "DELETE FROM setting WHERE key = 'notify_account_' || ?1",
            [account_id],
        )?;
        self.conn
            .execute("DELETE FROM account WHERE id = ?1", [account_id])?;
        Ok(())
    }

    /// Fetch an account by email, or `None` if absent.
    pub fn account_by_email(&self, email: &str) -> Result<Option<Account>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, email, display_name FROM account WHERE email = ?1",
                [email],
                |r| {
                    Ok(Account {
                        id: r.get(0)?,
                        email: r.get(1)?,
                        display_name: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// Fetch an account by id, or `None` if absent.
    pub fn account_by_id(&self, account_id: i64) -> Result<Option<Account>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, email, display_name FROM account WHERE id = ?1",
                [account_id],
                |r| {
                    Ok(Account {
                        id: r.get(0)?,
                        email: r.get(1)?,
                        display_name: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// All accounts, ordered by id.
    pub fn list_accounts(&self) -> Result<Vec<Account>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, email, display_name FROM account ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(Account {
                id: r.get(0)?,
                email: r.get(1)?,
                display_name: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Add a folder under an account; returns its id. Errors if it already exists.
    pub fn add_folder(&self, account_id: i64, name: &str) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO folder (account_id, name) VALUES (?1, ?2)",
            (account_id, name),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert a folder if absent (idempotent — for re-syncing the folder list); returns the
    /// folder's id whether it was just inserted or already present.
    pub fn upsert_folder(&self, account_id: i64, name: &str) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO folder (account_id, name) VALUES (?1, ?2) \
             ON CONFLICT(account_id, name) DO NOTHING",
            (account_id, name),
        )?;
        Ok(self.conn.query_row(
            "SELECT id FROM folder WHERE account_id = ?1 AND name = ?2",
            (account_id, name),
            |r| r.get(0),
        )?)
    }

    /// Upsert a folder **with the role the server gave it** (RFC 6154 SPECIAL-USE). Used by the folder
    /// listing, which is the only place that knows the roles.
    ///
    /// The role is refreshed on every listing, because the server is its authority: a user who marks a
    /// different folder as their Drafts folder in webmail must see that here on the next sync. Passing
    /// `None` therefore *clears* it — [`Self::upsert_folder`] (which every other caller uses, knowing
    /// only a name) deliberately leaves it alone, so syncing a folder can't blank its role.
    ///
    /// # Errors
    /// The upsert failing (a corrupt or unreadable database).
    pub fn upsert_folder_with_role(
        &self,
        account_id: i64,
        name: &str,
        role: Option<&str>,
    ) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO folder (account_id, name, role) VALUES (?1, ?2, ?3) \
             ON CONFLICT(account_id, name) DO UPDATE SET role = excluded.role",
            (account_id, name, role),
        )?;
        Ok(self.conn.query_row(
            "SELECT id FROM folder WHERE account_id = ?1 AND name = ?2",
            (account_id, name),
            |r| r.get(0),
        )?)
    }

    /// Remove this account's folders whose name is **not** in `keep` (their messages cascade). Used
    /// to reconcile the local folder list with the server after folder create/rename/delete (ORG-6).
    pub fn prune_folders(&self, account_id: i64, keep: &[String]) -> Result<(), StoreError> {
        for f in self.folders_for_account(account_id)? {
            // Local-only folders (e.g. "Saved", which holds opened .eml files) never exist on the
            // server, so the server's folder list wouldn't list them — keep them regardless.
            if is_local_folder(&f.name) {
                continue;
            }
            if !keep.iter().any(|k| k == &f.name) {
                self.conn
                    .execute("DELETE FROM folder WHERE id = ?1", [f.id])?;
            }
        }
        Ok(())
    }

    /// Folders for an account in a conventional order: **Inbox first**, then the common special
    /// folders, then everything else alphabetically (case-insensitive). Inbox-first also makes the
    /// app open to the Inbox (it shows the first folder), not whatever sorts first by name.
    pub fn folders_for_account(&self, account_id: i64) -> Result<Vec<Folder>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, account_id, name, role FROM folder WHERE account_id = ?1")?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(Folder {
                id: r.get(0)?,
                account_id: r.get(1)?,
                name: r.get(2)?,
                role: r.get(3)?,
            })
        })?;
        let mut folders = rows.collect::<Result<Vec<_>, _>>()?;
        folders.sort_by(|a, b| {
            folder_rank(&a.name, a.role.as_deref())
                .cmp(&folder_rank(&b.name, b.role.as_deref()))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(folders)
    }

    /// Unread (`seen = 0`) message count per folder for an account — for the rail's folder counts.
    /// Only folders with a non-zero count appear in the result.
    pub fn folder_unread_counts(&self, account_id: i64) -> Result<Vec<(i64, i64)>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT folder_id, COUNT(*) FROM message \
             WHERE account_id = ?1 AND seen = 0 AND pending_move IS NULL \
             AND (snoozed_until IS NULL OR snoozed_until <= unixepoch()) \
             GROUP BY folder_id",
        )?;
        let rows = stmt.query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The total unread across **every account's INBOX** (NOTIF-3) — the number for the window badge.
    ///
    /// Inbox-scoped on purpose: mail a server-side rule filed straight into a folder, and things
    /// sitting in Junk, are not what "you have unread mail" means to a person glancing at the taskbar.
    /// `INBOX` is IMAP's one reserved folder name (RFC 3501), matched case-insensitively.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn total_inbox_unread(&self) -> Result<i64, StoreError> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM message m JOIN folder f ON f.id = m.folder_id \
             WHERE m.seen = 0 AND f.name = 'INBOX' COLLATE NOCASE AND m.pending_move IS NULL \
             AND (m.snoozed_until IS NULL OR m.snoozed_until <= unixepoch())",
            [],
            |r| r.get(0),
        )?)
    }

    /// Insert or update a message envelope, keyed by `(account_id, folder_id, uid)`. On re-sync the
    /// envelope fields and seen flag are refreshed. `flagged`, `has_attachments`, and `snippet` are
    /// **not** overwritten on conflict: `flagged` is local state and the other two are body-derived
    /// (owned by `store_body`), so an envelope-only re-sync must not wipe them. They are set only on
    /// first insert (to defaults).
    pub fn upsert_message(
        &self,
        account_id: i64,
        folder_id: i64,
        m: &NewMessage,
    ) -> Result<i64, StoreError> {
        // `notified` is deliberately absent from the ON CONFLICT update list: a re-sync of a message
        // we have already announced must never make it news again.
        self.conn.execute(
            "INSERT INTO message \
             (account_id, folder_id, uid, message_id, in_reply_to, subject, from_name, from_addr, \
              to_addrs, cc_addrs, date, seen, flagged, has_attachments, snippet, notified, filtered) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17) \
             ON CONFLICT(account_id, folder_id, uid) DO UPDATE SET \
               message_id = excluded.message_id, in_reply_to = excluded.in_reply_to, \
               subject = excluded.subject, from_name = excluded.from_name, \
               from_addr = excluded.from_addr, to_addrs = excluded.to_addrs, \
               cc_addrs = excluded.cc_addrs, date = excluded.date, seen = excluded.seen",
            // `params!` (not a tuple): rusqlite's tuple `Params` impls stop at 16, and this is 17.
            rusqlite::params![
                account_id,
                folder_id,
                m.uid,
                m.message_id,
                m.in_reply_to,
                m.subject,
                m.from_name,
                m.from_addr,
                m.to_addrs,
                m.cc_addrs,
                m.date,
                m.seen,
                m.flagged,
                m.has_attachments,
                m.snippet,
                !m.owed_notification, // the column is `notified` — the same fact, said the other way
                // `filtered`: genuinely-new mail is owed a rule pass (0); backfill/already-here is done
                // (1) — same signal as `notified`, and, like it, absent from the ON CONFLICT update so a
                // re-sync never re-filters a message. New rows only, so a rule can't act on a message twice.
                !m.owed_notification,
            ],
        )?;
        let id = match m.uid {
            // On conflict the row is UPDATEd (not inserted), so look the id up by its unique key.
            Some(uid) => self.conn.query_row(
                "SELECT id FROM message WHERE account_id = ?1 AND folder_id = ?2 AND uid = ?3",
                (account_id, folder_id, uid),
                |r| r.get(0),
            )?,
            // A NULL uid never conflicts, so the row was just inserted.
            None => self.conn.last_insert_rowid(),
        };
        self.index_message(id)?; // keep the search index in step (body is added later via store_body)
        Ok(id)
    }

    /// Message headers for a folder, newest first (by date), up to `limit`.
    /// Every message id in a folder, **oldest-first** and with **no exclusions** (snoozed mail included)
    /// — for a complete export/backup (SEC-4). Unlike the listing queries this is order-for-archive and
    /// leaves nothing out; a backup that hid snoozed mail wouldn't be a backup.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn folder_message_ids(&self, folder_id: i64) -> Result<Vec<i64>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM message WHERE folder_id = ?1 ORDER BY date ASC, id ASC")?;
        let rows = stmt.query_map([folder_id], |r| r.get(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// How many messages a folder holds — for the export to skip empty folders and know an account is
    /// empty before opening a save dialog, without reading every id into memory.
    pub fn folder_message_count(&self, folder_id: i64) -> Result<i64, StoreError> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM message WHERE folder_id = ?1",
            [folder_id],
            |r| r.get(0),
        )?)
    }

    pub fn messages_in_folder(
        &self,
        folder_id: i64,
        limit: i64,
    ) -> Result<Vec<MessageHeader>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, uid, message_id, in_reply_to, subject, from_name, from_addr, date, seen, \
             has_attachments, snippet, flagged \
             FROM message WHERE folder_id = ?1 AND pending_move IS NULL \
             AND (snoozed_until IS NULL OR snoozed_until <= unixepoch()) \
             ORDER BY date DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map((folder_id, limit), |r| {
            Ok(MessageHeader {
                id: r.get(0)?,
                uid: r.get(1)?,
                message_id: r.get(2)?,
                in_reply_to: r.get(3)?,
                subject: r.get(4)?,
                from_name: r.get(5)?,
                from_addr: r.get(6)?,
                to_addrs: None, // not needed for the listing (header_by_id reads them)
                cc_addrs: None,
                date: r.get(7)?,
                seen: r.get(8)?,
                has_attachments: r.get(9)?,
                snippet: r.get(10)?,
                flagged: r.get(11)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The messages we still owe the user a notification for (NOTIF-1), newest first.
    ///
    /// Unseen **and** unannounced. Read from the store rather than from a sync's diff, so a message the
    /// backfill happened to store — or one a sync fetched while notifications were switched off, or
    /// during quiet hours — is still owed, and is not lost to whichever writer got there first.
    /// Reading it elsewhere in the meantime settles the debt: `seen` messages are never news.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn pending_notifications(
        &self,
        folder_id: i64,
        limit: i64,
    ) -> Result<Vec<PendingNotification>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, from_name, from_addr, subject FROM message \
             WHERE folder_id = ?1 AND notified = 0 AND seen = 0 AND pending_move IS NULL \
             ORDER BY date DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map((folder_id, limit), |r| {
            Ok(PendingNotification {
                id: r.get(0)?,
                from_name: r.get(1)?,
                from_addr: r.get(2)?,
                subject: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// How many messages this folder owes a notification for, and the highest id among them.
    ///
    /// The **count** must come from here, not from the sample we build the notification's text out of:
    /// summarising 50 of 300 owed messages as "50 new messages" is a lie, and settling only those 50
    /// means the next sweep raises another "50 new messages" five minutes later, about older mail. The
    /// number is the whole point of collapsing.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn pending_notification_summary(
        &self,
        folder_id: i64,
    ) -> Result<(i64, Option<i64>), StoreError> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*), MAX(id) FROM message \
             WHERE folder_id = ?1 AND notified = 0 AND seen = 0 AND pending_move IS NULL",
            [folder_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    /// The RFC `Message-ID`s of the messages this folder still owes a notification for.
    ///
    /// Read **before** a UIDVALIDITY reset clears the folder, so the debt can be carried across the
    /// rebuild — the server invalidating its UIDs is not the user being told about their mail.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn owed_message_ids(&self, folder_id: i64) -> Result<Vec<String>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT message_id FROM message \
             WHERE folder_id = ?1 AND notified = 0 AND seen = 0 AND message_id IS NOT NULL \
             AND pending_move IS NULL",
        )?;
        let rows = stmt.query_map([folder_id], |r| r.get(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Re-owe a notification for these messages, by `Message-ID` — the other half of carrying a debt
    /// across a UIDVALIDITY reset. Messages the user has since read elsewhere stay settled.
    ///
    /// # Errors
    /// The update failing (a corrupt or unreadable database).
    pub fn restore_owed(&self, folder_id: i64, message_ids: &[String]) -> Result<(), StoreError> {
        for mid in message_ids {
            self.conn.execute(
                "UPDATE message SET notified = 0 \
                 WHERE folder_id = ?1 AND message_id = ?2 AND seen = 0",
                (folder_id, mid),
            )?;
        }
        Ok(())
    }

    /// Settle the debt for everything in this folder up to and including `max_id` — one statement, so a
    /// half-marked batch can't leave some messages to be announced twice.
    ///
    /// Bounded by an id rather than "everything owed" because mail keeps arriving: a message stored
    /// *while* the notification was being raised has a higher id, was not in the notification, and must
    /// keep its debt.
    ///
    /// # Errors
    /// The update failing (a corrupt or unreadable database).
    pub fn mark_notified_through(&self, folder_id: i64, max_id: i64) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            "UPDATE message SET notified = 1 WHERE folder_id = ?1 AND id <= ?2 AND notified = 0",
            (folder_id, max_id),
        )?)
    }

    /// Record that the user has been told about these messages — the debt is settled, once.
    ///
    /// Called **after** the notification is actually raised, so a crash between the two costs a
    /// repeated notification rather than a silently swallowed one. (Of those two, only one loses mail.)
    ///
    /// # Errors
    /// The update failing (a corrupt or unreadable database).
    pub fn mark_notified(&self, ids: &[i64]) -> Result<(), StoreError> {
        for id in ids {
            self.conn
                .execute("UPDATE message SET notified = 1 WHERE id = ?1", [id])?;
        }
        Ok(())
    }

    /// The drafts sitting in a provider's Drafts folder, newest first — everything the Drafts list
    /// needs about them, in **one** query.
    ///
    /// Not `messages_in_folder` + `header_by_id` + `body_for` per row: that's three reads per draft,
    /// and the last one pulls the whole body through SQLCipher just to ask whether it has an HTML
    /// part. This asks the database that question instead (`html IS NOT NULL`), and never reads a body.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn drafts_in_folder(
        &self,
        folder_id: i64,
        limit: i64,
    ) -> Result<Vec<FolderDraft>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.message_id, m.to_addrs, m.cc_addrs, m.subject, m.snippet, m.date, \
             EXISTS(SELECT 1 FROM body b WHERE b.message_id = m.id AND b.html IS NOT NULL) \
             FROM message m WHERE m.folder_id = ?1 ORDER BY m.date DESC, m.id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map((folder_id, limit), |r| {
            Ok(FolderDraft {
                id: r.get(0)?,
                message_id: r.get(1)?,
                to_addrs: r.get(2)?,
                cc_addrs: r.get(3)?,
                subject: r.get(4)?,
                snippet: r.get(5)?,
                date: r.get(6)?,
                formatted: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Delete an account's local row for a message with this RFC 5322 `Message-ID`. Returns how many
    /// rows went (0 if we don't hold it).
    ///
    /// Used when a draft that had been mirrored to the provider is deleted: the mirrored copy is also
    /// a **message row** in the synced Drafts folder, and leaving it there would resurrect the draft
    /// the user just deleted — as an "On your provider" row, still resumable.
    ///
    /// # Errors
    /// The delete failing (a corrupt or unreadable database).
    pub fn delete_message_by_message_id(
        &self,
        account_id: i64,
        message_id: &str,
    ) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            "DELETE FROM message WHERE account_id = ?1 AND message_id = ?2",
            (account_id, message_id),
        )?)
    }

    /// Recent messages across **every** account's INBOX, newest first — for the merged "All inboxes"
    /// view. Each row is paired with its account id so the UI can tag it. (IMAP's inbox is always the
    /// folder literally named `INBOX`, so that's what "inbox" means here.)
    pub fn messages_in_all_inboxes(
        &self,
        limit: i64,
    ) -> Result<Vec<(MessageHeader, i64)>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.uid, m.message_id, m.in_reply_to, m.subject, m.from_name, m.from_addr, \
             m.date, m.seen, m.has_attachments, m.snippet, m.flagged, m.account_id \
             FROM message m JOIN folder f ON f.id = m.folder_id \
             WHERE f.name = 'INBOX' COLLATE NOCASE AND m.pending_move IS NULL \
             AND (m.snoozed_until IS NULL OR m.snoozed_until <= unixepoch()) \
             ORDER BY m.date DESC, m.id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| Ok((header_from_row(r)?, r.get::<_, i64>(12)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Address suggestions for autocomplete (SEND-9): distinct senders seen in this account's mail
    /// whose address starts with `prefix` (case-insensitive), alphabetically, up to `limit`.
    pub fn suggest_addresses(
        &self,
        account_id: i64,
        prefix: &str,
        limit: i64,
    ) -> Result<Vec<String>, StoreError> {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            return Ok(Vec::new());
        }
        // Escape LIKE wildcards so a literal % / _ in the prefix doesn't match everything.
        let escaped = prefix
            .to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("{escaped}%");
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT from_addr FROM message \
             WHERE account_id = ?1 AND from_addr IS NOT NULL \
               AND lower(from_addr) LIKE ?2 ESCAPE '\\' \
             ORDER BY from_addr LIMIT ?3",
        )?;
        let rows = stmt.query_map((account_id, pattern, limit), |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// A single message header by its store-row id (for reply/forward), or `None`.
    pub fn header_by_id(&self, id: i64) -> Result<Option<MessageHeader>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, uid, message_id, in_reply_to, subject, from_name, from_addr, date, \
                 seen, has_attachments, snippet, to_addrs, cc_addrs, flagged FROM message WHERE id = ?1",
                [id],
                |r| {
                    Ok(MessageHeader {
                        id: r.get(0)?,
                        uid: r.get(1)?,
                        message_id: r.get(2)?,
                        in_reply_to: r.get(3)?,
                        subject: r.get(4)?,
                        from_name: r.get(5)?,
                        from_addr: r.get(6)?,
                        to_addrs: r.get(11)?,
                        cc_addrs: r.get(12)?,
                        date: r.get(7)?,
                        seen: r.get(8)?,
                        has_attachments: r.get(9)?,
                        snippet: r.get(10)?,
                        flagged: r.get(13)?,
                    })
                },
            )
            .optional()?)
    }

    /// The store-row id of a message identified by its IMAP UID, or `None`.
    pub fn message_id_by_uid(
        &self,
        account_id: i64,
        folder_id: i64,
        uid: i64,
    ) -> Result<Option<i64>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM message WHERE account_id = ?1 AND folder_id = ?2 AND uid = ?3",
                (account_id, folder_id, uid),
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Store a message's body and update its `snippet`/`has_attachments`, atomically. Idempotent.
    pub fn store_body(
        &self,
        message_id: i64,
        plain: Option<&str>,
        html: Option<&str>,
        snippet: Option<&str>,
        has_attachments: bool,
    ) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO body (message_id, plain, html) VALUES (?1, ?2, ?3) \
             ON CONFLICT(message_id) DO UPDATE SET plain = excluded.plain, html = excluded.html",
            (message_id, plain, html),
        )?;
        tx.execute(
            "UPDATE message SET snippet = ?2, has_attachments = ?3 WHERE id = ?1",
            (message_id, snippet, has_attachments),
        )?;
        tx.commit()?;
        self.index_message(message_id)?; // re-index now that the body text is available
        Ok(())
    }

    /// Set a folder's IMAP UIDVALIDITY.
    /// Whether this folder has ever completed a sync (NOTIF-1). Until it has, everything in it looks
    /// "new", so nothing in it is worth announcing — see `sync::should_announce`.
    pub fn folder_primed(&self, folder_id: i64) -> Result<bool, StoreError> {
        Ok(self.conn.query_row(
            "SELECT primed FROM folder WHERE id = ?1",
            [folder_id],
            |r| r.get::<_, i64>(0),
        )? != 0)
    }

    /// Record that a folder has completed a sync — or, with `false`, that it must be primed again
    /// (the server reset UIDVALIDITY, so every message looks new once more).
    pub fn set_folder_primed(&self, folder_id: i64, primed: bool) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE folder SET primed = ?2 WHERE id = ?1",
            (folder_id, i64::from(primed)),
        )?;
        Ok(())
    }

    pub fn set_folder_uidvalidity(
        &self,
        folder_id: i64,
        uid_validity: i64,
    ) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE folder SET uid_validity = ?2 WHERE id = ?1",
            (folder_id, uid_validity),
        )?;
        Ok(())
    }

    /// A folder's stored UIDVALIDITY, or `None` if it has never been synced.
    pub fn folder_uidvalidity(&self, folder_id: i64) -> Result<Option<i64>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT uid_validity FROM folder WHERE id = ?1",
                [folder_id],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten())
    }

    /// All message UIDs stored in a folder (UID-less rows are skipped).
    pub fn uids_in_folder(&self, folder_id: i64) -> Result<Vec<i64>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT uid FROM message WHERE folder_id = ?1 AND uid IS NOT NULL")?;
        let rows = stmt.query_map([folder_id], |r| r.get(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Every held message's `(uid, seen, flagged)` in a folder that is **safe to reconcile** against the
    /// server — the local side of the SYNC-5 flag pull.
    ///
    /// Messages with a **pending local change** (`flags_dirty = 1`) are deliberately excluded: the user
    /// touched their flag here and the write-back to the server may not have landed, so letting the
    /// server's (stale) view win would undo the user's action. Once the write-back confirms, the row is
    /// no longer dirty and the next pull includes it — finding local and server already agree.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn flags_in_folder(&self, folder_id: i64) -> Result<Vec<(i64, bool, bool)>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT uid, seen, flagged FROM message \
             WHERE folder_id = ?1 AND uid IS NOT NULL AND flags_dirty = 0",
        )?;
        let rows = stmt.query_map([folder_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The flag changes this account still owes the server — every message with an unconfirmed local
    /// read/star change (SYNC-5), as `(message_id, folder_name, uid, seen, flagged)`.
    ///
    /// This IS the durable offline write-back queue: a flag change is written locally and marked dirty,
    /// and stays dirty (survives restarts) until its write-back to the server confirms. The scheduler
    /// drains this every sweep, so a change made offline reaches the server the next time we're online,
    /// rather than being lost when the one fire-and-forget attempt failed.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn pending_flag_writebacks(
        &self,
        account_id: i64,
    ) -> Result<Vec<PendingFlagWriteback>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, f.name, m.uid, m.seen, m.flagged \
             FROM message m JOIN folder f ON f.id = m.folder_id \
             WHERE m.account_id = ?1 AND m.flags_dirty = 1 AND m.uid IS NOT NULL",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(PendingFlagWriteback {
                id: r.get(0)?,
                folder: r.get(1)?,
                uid: r.get(2)?,
                seen: r.get(3)?,
                flagged: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Mark a message's local flag change as confirmed on the server (SYNC-5) — but **only if its flags
    /// still match what was pushed** (`seen`/`flagged`).
    ///
    /// The write-back worker reads a message's flags, pushes them, then calls this. If the user changed
    /// the flags again in between (read, then immediately marked unread), clearing unconditionally would
    /// settle the debt for a value the server no longer reflects — and the SYNC-5 pull would then revert
    /// the user's *newer* change. The condition makes the clear a no-op in that case, so the message
    /// stays queued and the next flush pushes the current value. Returns whether it cleared.
    ///
    /// # Errors
    /// The update failing (a corrupt or unreadable database).
    pub fn clear_flags_dirty(
        &self,
        message_id: i64,
        seen: bool,
        flagged: bool,
    ) -> Result<bool, StoreError> {
        let n = self.conn.execute(
            "UPDATE message SET flags_dirty = 0 WHERE id = ?1 AND seen = ?2 AND flagged = ?3",
            (message_id, seen, flagged),
        )?;
        Ok(n > 0)
    }

    /// Apply server flag changes to the messages we hold, by UID (SYNC-5) — the write half of the
    /// reconciliation. Atomic; a no-op for an empty list. Only `seen`/`flagged` move — never the
    /// envelope, the body, or `notified` (a message read elsewhere settles its own notification debt
    /// simply by becoming `seen`, which `pending_notifications` already filters on).
    ///
    /// # Errors
    /// The update failing (a corrupt or unreadable database).
    pub fn apply_flag_changes(
        &self,
        folder_id: i64,
        changes: &[(i64, bool, bool)],
    ) -> Result<(), StoreError> {
        if changes.is_empty() {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "UPDATE message SET seen = ?2, flagged = ?3 WHERE folder_id = ?1 AND uid = ?4",
            )?;
            for &(uid, seen, flagged) in changes {
                stmt.execute((folder_id, seen, flagged, uid))?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Delete messages in a folder by UID (bodies cascade). No-op for an empty list; atomic.
    pub fn delete_messages_by_uid(&self, folder_id: i64, uids: &[i64]) -> Result<(), StoreError> {
        if uids.is_empty() {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare("DELETE FROM message WHERE folder_id = ?1 AND uid = ?2")?;
            for &uid in uids {
                stmt.execute((folder_id, uid))?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// UIDs of the most-recent `limit` messages in a folder that have **no** stored body yet —
    /// the set a sync should (re)fetch bodies for, so an interrupted body fetch self-heals (P6).
    pub fn uids_without_body(&self, folder_id: i64, limit: u32) -> Result<Vec<i64>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT m.uid FROM message m LEFT JOIN body b ON b.message_id = m.id \
             WHERE m.folder_id = ?1 AND m.uid IS NOT NULL AND b.message_id IS NULL \
             ORDER BY m.date DESC, m.id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map((folder_id, limit), |r| r.get(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Delete every message in a folder (e.g. on a UIDVALIDITY reset). Bodies cascade.
    pub fn clear_folder(&self, folder_id: i64) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM message WHERE folder_id = ?1", [folder_id])?;
        Ok(())
    }

    /// The folder name + IMAP `uid` of a message, for server write-backs (SYNC-5). `None` if the
    /// message is gone or has no `uid` (local-only). Works regardless of the current view (e.g. when
    /// the message was opened from a cross-folder search result).
    pub fn message_location(&self, message_id: i64) -> Result<Option<(String, i64)>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT f.name, m.uid FROM message m JOIN folder f ON f.id = m.folder_id \
                 WHERE m.id = ?1 AND m.uid IS NOT NULL",
                [message_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// Queue a move to `target_folder` (OFF-4): mark the message pending so it vanishes from every
    /// listing at once, and let [`Self::pending_moves`] hand it to the server flush. Local + instant —
    /// no network — so a move made offline is recorded now and pushed on reconnect. Overwrites any
    /// earlier pending target (moving a still-queued message again just changes where it's headed).
    pub fn queue_move(&self, message_id: i64, target_folder: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE message SET pending_move = ?2 WHERE id = ?1",
            (message_id, target_folder),
        )?;
        Ok(())
    }

    /// Cancel a message's queued move (OFF-4): drop the marker so it reappears in its source folder.
    /// Used when the server **refuses** the move (an unknown target, or a uid that's already gone) —
    /// retrying such a move never succeeds, so rather than hide the message forever the flush un-hides
    /// it, and the user sees it back where it was. (A move that merely can't reach the server stays
    /// queued instead — that's the offline case, handled by leaving the marker be.)
    pub fn clear_pending_move(&self, message_id: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE message SET pending_move = NULL WHERE id = ?1",
            [message_id],
        )?;
        Ok(())
    }

    /// Every move waiting to reach this account's server (OFF-4), each with the source folder + uid the
    /// server call needs and the local id to delete once it lands. A message with no uid (a local-only
    /// Saved draft) can't be moved on a server it isn't on, so it's left out.
    pub fn pending_moves(&self, account_id: i64) -> Result<Vec<PendingMove>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, f.name, m.uid, m.pending_move \
             FROM message m JOIN folder f ON f.id = m.folder_id \
             WHERE m.account_id = ?1 AND m.pending_move IS NOT NULL AND m.uid IS NOT NULL \
             ORDER BY m.id ASC",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(PendingMove {
                id: r.get(0)?,
                source_folder: r.get(1)?,
                uid: r.get(2)?,
                target_folder: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Set a message's local read state. Server write-back of `\Seen` is the engine's job (SYNC-5).
    pub fn set_seen(&self, message_id: i64, seen: bool) -> Result<(), StoreError> {
        // Mark the flag dirty: this is a local change the server doesn't know about yet, so the SYNC-5
        // flag pull must not revert it until its write-back confirms (or, if that never happens, ever —
        // local intent wins). Cleared by [`Self::clear_flags_dirty`] on a successful write-back.
        self.conn.execute(
            "UPDATE message SET seen = ?2, flags_dirty = 1 WHERE id = ?1",
            (message_id, seen),
        )?;
        Ok(())
    }

    /// Set a message's local star/`\Flagged` state (ORG-4); the write-back to the server is the
    /// engine's job. Returns the message's IMAP `uid` (for that write-back), or `None`.
    pub fn set_flagged(&self, message_id: i64, flagged: bool) -> Result<Option<i64>, StoreError> {
        self.conn.execute(
            "UPDATE message SET flagged = ?2, flags_dirty = 1 WHERE id = ?1",
            (message_id, flagged),
        )?;
        Ok(self
            .conn
            .query_row("SELECT uid FROM message WHERE id = ?1", [message_id], |r| {
                r.get(0)
            })
            .optional()?
            .flatten())
    }

    /// (Re)build the FTS row for one message from its subject / sender / plain body (SEARCH-1).
    /// Called after an envelope upsert and again after the body arrives. Safe to call repeatedly;
    /// a missing message is a no-op. (Deletion is handled by the `message_fts_ad` trigger.)
    pub fn index_message(&self, message_id: i64) -> Result<(), StoreError> {
        let row = self
            .conn
            .query_row(
                "SELECT m.subject, m.from_name, m.from_addr, b.plain \
                 FROM message m LEFT JOIN body b ON b.message_id = m.id WHERE m.id = ?1",
                [message_id],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((subject, from_name, from_addr, plain)) = row else {
            return Ok(());
        };
        let sender = match (from_name, from_addr) {
            (Some(n), Some(a)) => format!("{n} {a}"),
            (Some(s), None) | (None, Some(s)) => s,
            (None, None) => String::new(),
        };
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM message_fts WHERE rowid = ?1", [message_id])?;
        tx.execute(
            "INSERT INTO message_fts (rowid, subject, sender, body) VALUES (?1, ?2, ?3, ?4)",
            (
                message_id,
                subject.unwrap_or_default(),
                sender,
                plain.unwrap_or_default(),
            ),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Rebuild the whole FTS index from the message table; returns how many were indexed.
    pub fn reindex_all(&self) -> Result<usize, StoreError> {
        let ids: Vec<i64> = {
            let mut stmt = self.conn.prepare("SELECT id FROM message")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for id in &ids {
            self.index_message(*id)?;
        }
        Ok(ids.len())
    }

    /// Full-text search this account's messages (SEARCH-1/2/3/4), best matches first. Supports the
    /// `from:` / `subject:` / `has:attachment` operators (see [`parse_search`]). Returns header rows
    /// (no `to_addrs`/`cc_addrs` — the listing doesn't need them).
    pub fn search_messages(
        &self,
        account_id: i64,
        query: &str,
        limit: i64,
    ) -> Result<Vec<MessageHeader>, StoreError> {
        let parsed = parse_search(query);
        match parsed.match_query {
            // Full-text terms present: rank by relevance, optionally filtered to has-attachments.
            // The `snippet(...)` column gives a short context window around the match (auto-picks the
            // best column; plain text, since the list renders it without markup) and replaces the
            // generic preview so a result shows *why* it matched.
            Some(match_query) => {
                let mut sql = String::from(
                    "SELECT m.id, m.uid, m.message_id, m.in_reply_to, m.subject, m.from_name, \
                     m.from_addr, m.date, m.seen, m.has_attachments, m.snippet, m.flagged, \
                     snippet(message_fts, -1, '', '', '…', 10) \
                     FROM message_fts JOIN message m ON m.id = message_fts.rowid \
                     WHERE message_fts MATCH ?1 AND m.account_id = ?2 AND m.pending_move IS NULL",
                );
                if parsed.require_attachment {
                    sql.push_str(" AND m.has_attachments = 1");
                }
                sql.push_str(" ORDER BY rank LIMIT ?3");
                let mut stmt = self.conn.prepare(&sql)?;
                let rows = stmt.query_map((match_query, account_id, limit), |r| {
                    let mut h = header_from_row(r)?;
                    let snip: String = r.get(12)?;
                    if !snip.is_empty() {
                        h.snippet = Some(snip); // show the match context, not the stored preview
                    }
                    Ok(h)
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            }
            // No full-text terms — only a `has:attachment` filter: list newest with attachments.
            None if parsed.require_attachment => {
                let mut stmt = self.conn.prepare(
                    "SELECT m.id, m.uid, m.message_id, m.in_reply_to, m.subject, m.from_name, \
                     m.from_addr, m.date, m.seen, m.has_attachments, m.snippet, m.flagged \
                     FROM message m \
                     WHERE m.account_id = ?1 AND m.has_attachments = 1 AND m.pending_move IS NULL \
                     ORDER BY m.date DESC, m.id DESC LIMIT ?2",
                )?;
                let rows = stmt.query_map((account_id, limit), header_from_row)?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            }
            None => Ok(Vec::new()),
        }
    }

    /// Like [`search_messages`] but across **all** accounts (SEARCH-5). Returns `(header, account_id)`
    /// so the caller knows which account each hit belongs to (to switch context when opening it).
    pub fn search_all_accounts(
        &self,
        query: &str,
        limit: i64,
    ) -> Result<Vec<(MessageHeader, i64)>, StoreError> {
        let parsed = parse_search(query);
        match parsed.match_query {
            Some(match_query) => {
                let mut sql = String::from(
                    "SELECT m.id, m.uid, m.message_id, m.in_reply_to, m.subject, m.from_name, \
                     m.from_addr, m.date, m.seen, m.has_attachments, m.snippet, m.flagged, \
                     snippet(message_fts, -1, '', '', '…', 10), m.account_id \
                     FROM message_fts JOIN message m ON m.id = message_fts.rowid \
                     WHERE message_fts MATCH ?1 AND m.pending_move IS NULL",
                );
                if parsed.require_attachment {
                    sql.push_str(" AND m.has_attachments = 1");
                }
                sql.push_str(" ORDER BY rank LIMIT ?2");
                let mut stmt = self.conn.prepare(&sql)?;
                let rows = stmt.query_map((match_query, limit), |r| {
                    let mut h = header_from_row(r)?;
                    let snip: String = r.get(12)?;
                    if !snip.is_empty() {
                        h.snippet = Some(snip);
                    }
                    Ok((h, r.get::<_, i64>(13)?))
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            }
            None if parsed.require_attachment => {
                let mut stmt = self.conn.prepare(
                    "SELECT m.id, m.uid, m.message_id, m.in_reply_to, m.subject, m.from_name, \
                     m.from_addr, m.date, m.seen, m.has_attachments, m.snippet, m.flagged, \
                     m.account_id FROM message m \
                     WHERE m.has_attachments = 1 AND m.pending_move IS NULL \
                     ORDER BY m.date DESC, m.id DESC LIMIT ?1",
                )?;
                let rows =
                    stmt.query_map([limit], |r| Ok((header_from_row(r)?, r.get::<_, i64>(12)?)))?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            }
            None => Ok(Vec::new()),
        }
    }

    /// The account a message belongs to (for routing a per-account server write-back). `None` if the
    /// message is gone.
    pub fn account_for_message(&self, message_id: i64) -> Result<Option<i64>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT account_id FROM message WHERE id = ?1",
                [message_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The account a folder belongs to, or `None` if the id is unknown — so an export can find the IMAP
    /// account to pull raw message bytes from (SEC-4).
    pub fn account_for_folder(&self, folder_id: i64) -> Result<Option<i64>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT account_id FROM folder WHERE id = ?1",
                [folder_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The IMAP uids of a folder's messages (those that have one — local-only Saved mail is skipped).
    /// For a **complete export** (SEC-4): the uids to pull the raw originals for, so attachments are
    /// included. Order matches [`Self::folder_message_ids`] (oldest-first) though callers use it as a set.
    pub fn folder_uids(&self, folder_id: i64) -> Result<Vec<u32>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT uid FROM message WHERE folder_id = ?1 AND uid IS NOT NULL \
             ORDER BY date ASC, id ASC",
        )?;
        let rows = stmt.query_map([folder_id], |r| r.get::<_, i64>(0))?;
        Ok(rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|u| u as u32)
            .collect())
    }

    /// Queue a composed message for delivery (SEND-10 outbox) — the built RFC 5322 bytes plus the
    /// envelope, so it can be handed to an SMTP server later. Returns the new outbox id.
    ///
    /// # Errors
    /// The insert failing (a corrupt or unreadable database).
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_outbox(
        &self,
        account_id: i64,
        mail_from: &str,
        recipients: &[String],
        subject: &str,
        raw: &[u8],
        server_draft: Option<(&str, &str)>,
        created_at: i64,
    ) -> Result<i64, StoreError> {
        let (draft_folder, draft_msgid) = server_draft.unzip();
        self.conn.execute(
            "INSERT INTO outbox \
             (account_id, mail_from, recipients, subject, raw, draft_folder, draft_msgid, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                account_id,
                mail_from,
                recipients.join(","),
                subject,
                raw,
                draft_folder,
                draft_msgid,
                created_at,
            ),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// The account's outbox messages **still worth retrying** (`failed = 0`), oldest first — the
    /// scheduler drains these. A `failed` one (the server rejected it) is left for the user to see.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn pending_outbox(&self, account_id: i64) -> Result<Vec<OutboxMessage>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, mail_from, recipients, subject, raw, draft_folder, draft_msgid FROM outbox \
             WHERE account_id = ?1 AND failed = 0 ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            let recipients: String = r.get(2)?;
            let folder: Option<String> = r.get(5)?;
            let msgid: Option<String> = r.get(6)?;
            let draft = folder.zip(msgid);
            Ok(OutboxMessage {
                id: r.get(0)?,
                mail_from: r.get(1)?,
                recipients: recipients.split(',').map(str::to_owned).collect(),
                subject: r.get(3)?,
                raw: r.get(4)?,
                server_draft: draft,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Remove a delivered message from the outbox (SEND-10). Idempotent.
    ///
    /// # Errors
    /// The delete failing (a corrupt or unreadable database).
    pub fn delete_outbox(&self, id: i64) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM outbox WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Mark an outbox message as permanently failed (the server rejected it) so it stops being retried
    /// and can be surfaced to the user (SEND-10).
    ///
    /// # Errors
    /// The update failing (a corrupt or unreadable database).
    pub fn mark_outbox_failed(&self, id: i64, error: &str) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE outbox SET failed = 1, last_error = ?2 WHERE id = ?1",
            (id, error),
        )?;
        Ok(())
    }

    /// Every outbox message across **all** accounts — for the outbox view, so the user can retry or
    /// discard one. Newest first. Carries display fields and status, not the raw bytes.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn list_outbox(&self) -> Result<Vec<OutboxItem>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, recipients, subject, failed, last_error FROM outbox \
             ORDER BY created_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            let recipients: String = r.get(1)?;
            let failed: i64 = r.get(3)?;
            Ok(OutboxItem {
                id: r.get(0)?,
                recipients: recipients.split(',').map(str::to_owned).collect(),
                subject: r.get(2)?,
                failed: failed != 0,
                last_error: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The account an outbox message belongs to, or `None` if it's gone — so a retry can flush the
    /// right account.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn outbox_account(&self, id: i64) -> Result<Option<i64>, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT account_id FROM outbox WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?)
    }

    /// The raw RFC 5322 bytes of one queued message (SEND-10 edit), or `None` if the row is gone.
    /// Used to reopen a rejected send in the composer — the bytes parse straight back into a form.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn outbox_raw(&self, id: i64) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT raw FROM outbox WHERE id = ?1", [id], |r| r.get(0))
            .optional()?)
    }

    /// Re-queue a failed outbox message (SEND-10): clear its failed mark and error so the next drain
    /// tries it again. A no-op for one that isn't failed.
    ///
    /// # Errors
    /// The update failing (a corrupt or unreadable database).
    pub fn retry_outbox(&self, id: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE outbox SET failed = 0, last_error = NULL WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    /// How many messages are waiting to send and how many have failed, across **all** accounts — for
    /// the UI's outbox indicator. `(queued, failed)`.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn outbox_counts(&self) -> Result<(i64, i64), StoreError> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(CASE WHEN failed = 0 THEN 1 ELSE 0 END), 0), \
                    COALESCE(SUM(CASE WHEN failed = 1 THEN 1 ELSE 0 END), 0) \
             FROM outbox",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    // --- Snooze (ORG-9) ---------------------------------------------------------------------------

    /// Snooze messages until `until` (a unix timestamp): hide them from their folder list and the
    /// unread badge until then. Idempotent per id; re-snoozing just moves the time.
    ///
    /// # Errors
    /// The update failing (a corrupt or unreadable database).
    pub fn snooze_messages(&self, ids: &[i64], until: i64) -> Result<(), StoreError> {
        let mut stmt = self
            .conn
            .prepare("UPDATE message SET snoozed_until = ?2 WHERE id = ?1")?;
        for &id in ids {
            stmt.execute((id, until))?;
        }
        Ok(())
    }

    /// Bring a snoozed message back now — clear its snooze so it reappears in its folder immediately.
    /// A no-op for one that isn't snoozed.
    ///
    /// # Errors
    /// The update failing (a corrupt or unreadable database).
    pub fn unsnooze_message(&self, id: i64) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE message SET snoozed_until = NULL WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    /// Resurface every snooze whose time has come: clear it, and re-owe a notification (`notified = 0`)
    /// **only for messages still unread** — so a message snoozed while read doesn't come back owing a
    /// spurious "new mail" alert the day it's marked unread again. An unread one re-enters the
    /// notification pipeline (NOTIF-1) and is announced as if it just arrived. Returns how many
    /// resurfaced, so the caller knows the list and badge are stale. Uses SQLite's clock, so "due"
    /// means due right now.
    ///
    /// # Errors
    /// The update failing (a corrupt or unreadable database).
    pub fn resurface_due_snoozes(&self) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            "UPDATE message SET snoozed_until = NULL, \
                    notified = CASE WHEN seen = 0 THEN 0 ELSE notified END \
             WHERE snoozed_until IS NOT NULL AND snoozed_until <= unixepoch()",
            [],
        )?)
    }

    /// The messages still snoozed for an account (resurface time in the future), soonest-first — for the
    /// Snoozed view.
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn snoozed_messages(&self, account_id: i64) -> Result<Vec<SnoozedItem>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, from_name, from_addr, subject, snoozed_until FROM message \
             WHERE account_id = ?1 AND snoozed_until IS NOT NULL AND snoozed_until > unixepoch() \
             ORDER BY snoozed_until ASC, id ASC",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(SnoozedItem {
                id: r.get(0)?,
                from_name: r.get(1)?,
                from_addr: r.get(2)?,
                subject: r.get(3)?,
                snoozed_until: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    // --- Rules / filters (ORG-8) ------------------------------------------------------------------

    /// Add a rule for an account (ORG-8). Appended to the end of the account's order (first-match-wins,
    /// so a new rule is lowest priority). Returns its id.
    ///
    /// # Errors
    /// The insert failing (a corrupt or unreadable database).
    #[allow(clippy::too_many_arguments)]
    pub fn add_rule(
        &self,
        account_id: i64,
        field: &str,
        pattern: &str,
        target_folder: Option<&str>,
        mark_read: bool,
        star: bool,
        created_at: i64,
    ) -> Result<i64, StoreError> {
        let position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM rule WHERE account_id = ?1",
            [account_id],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO rule \
             (account_id, field, pattern, target_folder, mark_read, star, position, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            (
                account_id,
                field,
                pattern,
                target_folder,
                mark_read,
                star,
                position,
                created_at,
            ),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// An account's rules, in evaluation order (ORG-8).
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn list_rules(&self, account_id: i64) -> Result<Vec<Rule>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, field, pattern, target_folder, mark_read, star FROM rule \
             WHERE account_id = ?1 ORDER BY position ASC, id ASC",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(Rule {
                id: r.get(0)?,
                account_id: r.get(1)?,
                field: r.get(2)?,
                pattern: r.get(3)?,
                target_folder: r.get(4)?,
                mark_read: r.get(5)?,
                star: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Delete a rule (ORG-8). Idempotent.
    ///
    /// # Errors
    /// The delete failing (a corrupt or unreadable database).
    pub fn delete_rule(&self, id: i64) -> Result<(), StoreError> {
        self.conn.execute("DELETE FROM rule WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Move a rule one step up (`up = true`) or down its account's evaluation order (ORG-8) — rules are
    /// first-match-wins, so this is how the user sets priority. Swaps the rule's `position` with its
    /// immediate neighbour. A no-op at the top/bottom edge, or for an unknown id.
    ///
    /// # Errors
    /// The query or swap failing (a corrupt or unreadable database).
    pub fn move_rule(&self, id: i64, up: bool) -> Result<(), StoreError> {
        let Some((account_id, position)) = self
            .conn
            .query_row(
                "SELECT account_id, position FROM rule WHERE id = ?1",
                [id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?
        else {
            return Ok(());
        };
        // The neighbour to swap with: the closest rule above (up) or below (down) in the order.
        let sql = if up {
            "SELECT id, position FROM rule WHERE account_id = ?1 AND position < ?2 ORDER BY position DESC LIMIT 1"
        } else {
            "SELECT id, position FROM rule WHERE account_id = ?1 AND position > ?2 ORDER BY position ASC LIMIT 1"
        };
        let Some((neighbour_id, neighbour_pos)) = self
            .conn
            .query_row(sql, (account_id, position), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .optional()?
        else {
            return Ok(()); // already at the edge
        };
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE rule SET position = ?2 WHERE id = ?1",
            (id, neighbour_pos),
        )?;
        tx.execute(
            "UPDATE rule SET position = ?2 WHERE id = ?1",
            (neighbour_id, position),
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The account's INBOX messages still awaiting a rule pass (`filtered = 0`), oldest-first, with the
    /// fields a rule condition tests (ORG-8).
    ///
    /// # Errors
    /// The query failing (a corrupt or unreadable database).
    pub fn unfiltered_inbox(&self, account_id: i64) -> Result<Vec<UnfilteredMessage>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, m.from_name, m.from_addr, m.subject, m.to_addrs \
             FROM message m JOIN folder f ON f.id = m.folder_id \
             WHERE m.account_id = ?1 AND f.name = 'INBOX' COLLATE NOCASE AND m.filtered = 0 \
             AND m.pending_move IS NULL \
             ORDER BY m.id ASC",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(UnfilteredMessage {
                id: r.get(0)?,
                from_name: r.get(1)?,
                from_addr: r.get(2)?,
                subject: r.get(3)?,
                to_addrs: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Mark a message as having had its rule pass (ORG-8): it won't be re-evaluated.
    ///
    /// # Errors
    /// The update failing (a corrupt or unreadable database).
    pub fn mark_filtered(&self, id: i64) -> Result<(), StoreError> {
        self.conn
            .execute("UPDATE message SET filtered = 1 WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Re-arm the account's whole INBOX for a rule pass (**Run on inbox now**, ORG-8): set `filtered = 0`
    /// on every INBOX message so the next `apply_rules` re-evaluates them all. Returns how many.
    ///
    /// # Errors
    /// The update failing (a corrupt or unreadable database).
    pub fn reset_inbox_filtered(&self, account_id: i64) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            "UPDATE message SET filtered = 0 \
             WHERE account_id = ?1 AND folder_id IN \
               (SELECT id FROM folder WHERE account_id = ?1 AND name = 'INBOX' COLLATE NOCASE)",
            [account_id],
        )?)
    }

    /// Remove a single message locally (optimistic archive/trash/move; body + attachments cascade).
    /// The server change is the engine's job; on failure a re-sync restores the row.
    pub fn delete_message(&self, message_id: i64) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM message WHERE id = ?1", [message_id])?;
        Ok(())
    }

    /// Delete every message in a folder locally (for "empty trash"); returns how many were removed.
    /// The server side is emptied separately. Cascades to bodies/attachments/FTS like `delete_message`.
    pub fn delete_folder_messages(&self, folder_id: i64) -> Result<usize, StoreError> {
        Ok(self
            .conn
            .execute("DELETE FROM message WHERE folder_id = ?1", [folder_id])?)
    }

    /// Rename a folder in place (ORG-6). Keeps the same `folder_id`, so the folder's messages stay
    /// attached (a delete-and-re-list would cascade them away). Returns how many rows changed (0 if
    /// no folder by that name).
    pub fn rename_folder(
        &self,
        account_id: i64,
        from: &str,
        to: &str,
    ) -> Result<usize, StoreError> {
        Ok(self.conn.execute(
            "UPDATE folder SET name = ?3 WHERE account_id = ?1 AND name = ?2",
            (account_id, from, to),
        )?)
    }

    /// Delete a folder row and, by `ON DELETE CASCADE`, all of its messages/bodies/attachments
    /// (ORG-6). For the local half of a server folder delete.
    pub fn delete_folder(&self, folder_id: i64) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM folder WHERE id = ?1", [folder_id])?;
        Ok(())
    }

    /// The stored body for a message, or `None` if no body is stored yet.
    pub fn body_for(&self, message_id: i64) -> Result<Option<StoredBody>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT plain, html FROM body WHERE message_id = ?1",
                [message_id],
                |r| {
                    Ok(StoredBody {
                        plain: r.get(0)?,
                        html: r.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    /// Replace the stored attachment metadata for a message (atomic; idempotent on re-sync).
    pub fn store_attachments(
        &self,
        message_id: i64,
        attachments: &[Attachment],
    ) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM attachment WHERE message_id = ?1", [message_id])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO attachment (message_id, filename, content_type, size_bytes) \
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for a in attachments {
                stmt.execute((message_id, &a.filename, &a.content_type, a.size))?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The attachment metadata stored for a message (in insertion order).
    pub fn attachments_for(&self, message_id: i64) -> Result<Vec<Attachment>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT filename, content_type, size_bytes FROM attachment \
             WHERE message_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([message_id], |r| {
            Ok(Attachment {
                filename: r.get(0)?,
                content_type: r.get(1)?,
                size: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    #[cfg(test)]
    fn table_names(&self) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    #[cfg(test)]
    fn user_version(&self) -> Result<i64, StoreError> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_create_all_tables_and_set_version() {
        let s = Store::open_in_memory().unwrap();
        let tables = s.table_names().unwrap();
        for t in ["account", "folder", "message", "body"] {
            assert!(tables.contains(&t.to_string()), "missing table {t}");
        }
        assert_eq!(s.user_version().unwrap(), MIGRATIONS.len() as i64);
    }

    #[test]
    fn migrate_is_idempotent() {
        let mut s = Store::open_in_memory().unwrap();
        let v = s.user_version().unwrap();
        s.migrate().unwrap(); // nothing pending
        assert_eq!(s.user_version().unwrap(), v);
        assert_eq!(
            s.table_names()
                .unwrap()
                .iter()
                .filter(|t| *t == "account")
                .count(),
            1
        );
    }

    #[test]
    fn accounts_insert_get_list() {
        let s = Store::open_in_memory().unwrap();
        let id = s.add_account("anna@example.com", Some("Anna")).unwrap();
        let got = s.account_by_email("anna@example.com").unwrap().unwrap();
        assert_eq!(got.id, id);
        assert_eq!(got.display_name.as_deref(), Some("Anna"));
        assert!(s.account_by_email("nobody@example.com").unwrap().is_none());
        s.add_account("bob@example.com", None).unwrap();
        assert_eq!(s.list_accounts().unwrap().len(), 2);
    }

    #[test]
    fn rejects_invalid_email() {
        let s = Store::open_in_memory().unwrap();
        assert!(matches!(
            s.add_account("not-an-email", None),
            Err(StoreError::InvalidEmail)
        ));
    }

    #[test]
    fn fresh_connection_to_migrated_file_skips_migrations() {
        let path =
            std::env::temp_dir().join(format!("geleit-store-roundtrip-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let s = Store::open(&path).unwrap();
            s.add_account("a@example.com", None).unwrap();
            assert_eq!(s.user_version().unwrap(), MIGRATIONS.len() as i64);
        }
        {
            // reopen: a fresh connection to the already-migrated file applies nothing new
            let s = Store::open(&path).unwrap();
            assert_eq!(s.user_version().unwrap(), MIGRATIONS.len() as i64);
            assert!(s.account_by_email("a@example.com").unwrap().is_some());
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn duplicate_email_is_error() {
        let s = Store::open_in_memory().unwrap();
        s.add_account("a@example.com", None).unwrap();
        assert!(s.add_account("a@example.com", None).is_err());
    }

    #[test]
    fn folder_unique_per_account_and_cascades() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        s.add_folder(acc, "INBOX").unwrap();
        assert!(s.add_folder(acc, "INBOX").is_err()); // UNIQUE(account_id, name)
        s.add_folder(acc, "Sent").unwrap();
        assert_eq!(s.folders_for_account(acc).unwrap().len(), 2);
        // foreign_keys = ON ⇒ deleting the account cascades to its folders
        s.conn
            .execute("DELETE FROM account WHERE id = ?1", [acc])
            .unwrap();
        assert_eq!(s.folders_for_account(acc).unwrap().len(), 0);
    }

    #[test]
    fn upsert_folder_is_idempotent_and_scoped() {
        let s = Store::open_in_memory().unwrap();
        let a = s.add_account("a@example.com", None).unwrap();
        let b = s.add_account("b@example.com", None).unwrap();
        let id1 = s.upsert_folder(a, "INBOX").unwrap();
        let id2 = s.upsert_folder(a, "INBOX").unwrap(); // again → same row, no error
        assert_eq!(id1, id2);
        assert_eq!(s.folders_for_account(a).unwrap().len(), 1);
        // same name under a different account is a distinct folder
        s.upsert_folder(b, "INBOX").unwrap();
        assert_eq!(s.folders_for_account(b).unwrap().len(), 1);
    }

    #[test]
    fn upsert_message_inserts_then_updates() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();
        let m = NewMessage {
            uid: Some(10),
            subject: Some("Hi".to_owned()),
            seen: false,
            ..Default::default()
        };
        let id1 = s.upsert_message(acc, fld, &m).unwrap();
        // re-sync same uid with seen flipped → same row, updated, not duplicated
        let m2 = NewMessage {
            uid: Some(10),
            subject: Some("Hi".to_owned()),
            seen: true,
            ..Default::default()
        };
        let id2 = s.upsert_message(acc, fld, &m2).unwrap();
        assert_eq!(id1, id2);
        let msgs = s.messages_in_folder(fld, 50).unwrap();
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].seen);
    }

    #[test]
    fn a_queued_move_hides_the_message_and_survives_a_resync() {
        // OFF-4: a move made offline is a `pending_move` marker. It must (1) hide the message from every
        // listing at once, (2) show up in `pending_moves` with the source folder + uid the server call
        // needs, and — the property that makes it safe — (3) survive a re-sync of the source folder,
        // where the message still is, rather than being un-hidden and re-shown.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let m = NewMessage {
            uid: Some(7),
            subject: Some("Hi".to_owned()),
            seen: false,
            ..Default::default()
        };
        let id = s.upsert_message(acc, inbox, &m).unwrap();
        assert_eq!(s.messages_in_folder(inbox, 50).unwrap().len(), 1);

        s.queue_move(id, "Archive").unwrap();
        // Hidden everywhere the user would see it — including search, which is a listing too.
        assert!(s.messages_in_folder(inbox, 50).unwrap().is_empty());
        assert!(s.messages_in_all_inboxes(50).unwrap().is_empty());
        assert_eq!(s.total_inbox_unread().unwrap(), 0);
        assert!(s.folder_unread_counts(acc).unwrap().is_empty());
        assert!(
            s.search_messages(acc, "Hi", 50).unwrap().is_empty(),
            "a queued move must hide the message from search too"
        );
        assert!(s.pending_notifications(inbox, 50).unwrap().is_empty());
        assert_eq!(s.pending_notification_summary(inbox).unwrap().0, 0);
        // Queued, with everything the server move needs.
        let pending = s.pending_moves(acc).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id);
        assert_eq!(pending[0].source_folder, "INBOX");
        assert_eq!(pending[0].uid, 7);
        assert_eq!(pending[0].target_folder, "Archive");

        // A re-sync of INBOX (the message is still there on the server until the move lands) must not
        // clear the marker: the row stays hidden and stays queued.
        let id2 = s.upsert_message(acc, inbox, &m).unwrap();
        assert_eq!(id2, id);
        assert!(s.messages_in_folder(inbox, 50).unwrap().is_empty());
        assert_eq!(s.pending_moves(acc).unwrap().len(), 1);
    }

    #[test]
    fn clearing_a_pending_move_brings_the_message_back() {
        // OFF-4 recovery: when the server refuses a move (unknown target / gone uid), the flush clears
        // the marker so the message reappears in its source folder rather than hiding behind a move that
        // can never complete.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let id = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(9),
                    subject: Some("back soon".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        s.queue_move(id, "Nowhere").unwrap();
        assert!(s.messages_in_folder(inbox, 50).unwrap().is_empty());
        s.clear_pending_move(id).unwrap();
        let back = s.messages_in_folder(inbox, 50).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, id);
        assert!(s.pending_moves(acc).unwrap().is_empty());
    }

    #[test]
    fn a_queued_move_is_left_out_of_notifications_and_the_rules_pass() {
        // A message the user files the instant it lands — before the notify sweep raises it, or the rule
        // pass sorts it — must not then get a popup or be moved somewhere else by a rule. So `pending_move`
        // hides it from the notification queue and from `unfiltered_inbox` too, not just the visible lists.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        // owed_notification = true ⇒ notified = 0 (owed a popup) AND filtered = 0 (owed a rule pass).
        let id = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(11),
                    message_id: Some("<m11@example.com>".to_owned()),
                    subject: Some("just arrived".to_owned()),
                    owed_notification: true,
                    ..Default::default()
                },
            )
            .unwrap();
        // Before the move: it's owed a notification and awaiting a rule pass.
        assert_eq!(s.pending_notifications(inbox, 50).unwrap().len(), 1);
        assert_eq!(s.owed_message_ids(inbox).unwrap().len(), 1);
        assert_eq!(s.unfiltered_inbox(acc).unwrap().len(), 1);

        s.queue_move(id, "Archive").unwrap();
        // After the move: dropped from every one of them.
        assert!(s.pending_notifications(inbox, 50).unwrap().is_empty());
        assert_eq!(s.pending_notification_summary(inbox).unwrap().0, 0);
        assert!(s.owed_message_ids(inbox).unwrap().is_empty());
        assert!(s.unfiltered_inbox(acc).unwrap().is_empty());
    }

    #[test]
    fn folder_message_count_counts_all_messages() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        assert_eq!(s.folder_message_count(inbox).unwrap(), 0);
        for uid in [Some(1i64), Some(2), None] {
            s.upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        assert_eq!(s.folder_message_count(inbox).unwrap(), 3); // local-only (no uid) counts too
        assert_eq!(s.folder_message_count(9999).unwrap(), 0); // unknown folder
    }

    #[test]
    fn folder_uids_lists_server_messages_and_account_for_folder_resolves() {
        // SEC-4 export: to pull raw originals we need the folder's account and its messages' uids —
        // local-only mail (no uid) is skipped since it isn't on any server to fetch from.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        assert_eq!(s.account_for_folder(inbox).unwrap(), Some(acc));
        assert_eq!(s.account_for_folder(9999).unwrap(), None);

        for (uid, date) in [(Some(5i64), 200i64), (Some(3), 100)] {
            s.upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid,
                    date: Some(date),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        // A local-only Saved message (no uid) must not appear — nothing to fetch from a server.
        s.upsert_message(
            acc,
            inbox,
            &NewMessage {
                uid: None,
                date: Some(300),
                ..Default::default()
            },
        )
        .unwrap();
        // Oldest-first by date, uids only.
        assert_eq!(s.folder_uids(inbox).unwrap(), vec![3u32, 5]);
    }

    #[test]
    fn a_local_only_message_is_never_a_pending_move() {
        // A Saved draft (no uid) isn't on any server, so it can't be a server move — `pending_moves`
        // must skip it, or the flush would try to move a message the server has never seen.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let drafts = s.upsert_folder(acc, "Drafts").unwrap();
        let id = s
            .upsert_message(
                acc,
                drafts,
                &NewMessage {
                    uid: None,
                    subject: Some("wip".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        s.queue_move(id, "Archive").unwrap();
        assert!(s.pending_moves(acc).unwrap().is_empty());
    }

    #[test]
    fn being_told_about_a_message_is_a_fact_that_survives_the_next_sync() {
        // The bug this exists to prevent: "is this news?" used to be a diff against the store, so
        // whichever writer stored the message first ate the signal — a message the backfill swept up
        // was never announced, because by the next sync it was no longer absent.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let msg = |uid: i64, owed: bool, seen: bool| NewMessage {
            uid: Some(uid),
            from_name: Some("Alice".to_owned()),
            subject: Some("Lunch?".to_owned()),
            seen,
            owed_notification: owed,
            ..Default::default()
        };

        let owed = s.upsert_message(acc, inbox, &msg(1, true, false)).unwrap();
        s.upsert_message(acc, inbox, &msg(2, false, false)).unwrap(); // already accounted for (old mail)
        s.upsert_message(acc, inbox, &msg(3, true, true)).unwrap(); // read elsewhere → never news

        let pending = s.pending_notifications(inbox, 10).unwrap();
        assert_eq!(pending.len(), 1, "only the unseen, unannounced one is owed");
        assert_eq!(pending[0].id, owed);
        assert_eq!(pending[0].from_name.as_deref(), Some("Alice"));
        assert_eq!(pending[0].subject.as_deref(), Some("Lunch?"));

        // A re-sync of the same message — flags change, the envelope is re-fetched — must NOT make it
        // news again, or every sync would re-announce the whole inbox.
        s.upsert_message(acc, inbox, &msg(1, true, false)).unwrap();
        s.mark_notified(&[owed]).unwrap();
        s.upsert_message(acc, inbox, &msg(1, true, false)).unwrap();
        assert!(
            s.pending_notifications(inbox, 10).unwrap().is_empty(),
            "told once, never again"
        );
    }

    #[test]
    fn a_debt_survives_the_server_reinventing_its_uids() {
        // A UIDVALIDITY reset clears the folder and re-fetches it, and the re-fetch announces nothing
        // (the folder is unprimed again — else every message in the inbox would pop up). Without
        // carrying the debt across, mail we owed a notification for and hadn't raised yet — held
        // through quiet hours, say — would be silently written off. The server renumbering its
        // mailbox is not the user being told about their mail.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let msg = |uid: i64, mid: &str, owed: bool, seen: bool| NewMessage {
            uid: Some(uid),
            message_id: Some(mid.to_owned()),
            owed_notification: owed,
            seen,
            ..Default::default()
        };
        s.upsert_message(acc, inbox, &msg(1, "<owed@x>", true, false))
            .unwrap();
        s.upsert_message(acc, inbox, &msg(2, "<known@x>", false, false))
            .unwrap();

        // What we'd read before clearing the folder…
        let carried = s.owed_message_ids(inbox).unwrap();
        assert_eq!(carried, vec!["<owed@x>".to_owned()], "only the unpaid one");

        // …the server reinvents its UIDs, so the folder is rebuilt and everything comes back as
        // "already accounted for" (an unprimed folder announces nothing).
        s.clear_folder(inbox).unwrap();
        s.upsert_message(acc, inbox, &msg(101, "<owed@x>", false, false))
            .unwrap();
        s.upsert_message(acc, inbox, &msg(102, "<known@x>", false, false))
            .unwrap();
        assert!(s.pending_notifications(inbox, 10).unwrap().is_empty());

        // …and the debt is carried across.
        s.restore_owed(inbox, &carried).unwrap();
        let owed = s.pending_notifications(inbox, 10).unwrap();
        assert_eq!(owed.len(), 1);
        assert_eq!(s.pending_notification_summary(inbox).unwrap().0, 1);

        // A message the user read elsewhere in the meantime stays settled — they know about it.
        s.set_seen(owed[0].id, true).unwrap();
        s.restore_owed(inbox, &carried).unwrap();
        assert!(s.pending_notifications(inbox, 10).unwrap().is_empty());
    }

    #[test]
    fn settling_a_batch_leaves_the_mail_that_arrived_while_we_were_telling_them() {
        // The notification named the mail up to `max_id`. Mail stored *while* it was being raised has a
        // higher id, was not in it, and must keep its debt — or it is announced to nobody, ever.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let new = |uid: i64| NewMessage {
            uid: Some(uid),
            owed_notification: true,
            ..Default::default()
        };
        let first = s.upsert_message(acc, inbox, &new(1)).unwrap();
        let second = s.upsert_message(acc, inbox, &new(2)).unwrap();
        let (count, max) = s.pending_notification_summary(inbox).unwrap();
        assert_eq!((count, max), (2, Some(second)));

        // …a third lands while the notification is on screen.
        let third = s.upsert_message(acc, inbox, &new(3)).unwrap();
        assert_eq!(s.mark_notified_through(inbox, second).unwrap(), 2);
        let left = s.pending_notifications(inbox, 10).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, third, "the latecomer is still owed");
        assert_ne!(left[0].id, first);
    }

    #[test]
    fn removing_an_account_takes_its_notification_setting_with_it() {
        // SQLite reuses the ids of deleted rows, and the per-account switches are keyed by id. Leave one
        // behind and the NEXT account added inherits it: notifications silently off, and (with one
        // account) no switch in the UI to find out why.
        let s = Store::open_in_memory().unwrap();
        let doomed = s.add_account("a@example.com", None).unwrap();
        s.set_setting(&format!("notify_account_{doomed}"), "0")
            .unwrap();
        s.set_setting("notify", "1").unwrap();

        s.delete_account(doomed).unwrap();
        assert_eq!(
            s.get_setting(&format!("notify_account_{doomed}")).unwrap(),
            None,
            "the setting went with the account"
        );
        assert_eq!(
            s.get_setting("notify").unwrap().as_deref(),
            Some("1"),
            "…and took nothing else with it"
        );

        let reborn = s.add_account("b@example.com", None).unwrap();
        assert_eq!(reborn, doomed, "SQLite really does hand the id back");
        assert_eq!(
            s.get_setting(&format!("notify_account_{reborn}")).unwrap(),
            None,
            "a new account starts with a clean slate, not the dead one's silence"
        );
    }

    #[test]
    fn reading_a_message_elsewhere_settles_the_debt_and_the_notification_is_scoped_to_its_folder() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let other = s.upsert_folder(acc, "Work").unwrap();
        let new = |uid: i64| NewMessage {
            uid: Some(uid),
            owed_notification: true,
            seen: false,
            ..Default::default()
        };
        let id = s.upsert_message(acc, inbox, &new(1)).unwrap();
        s.upsert_message(acc, other, &new(2)).unwrap();

        assert_eq!(s.pending_notifications(inbox, 10).unwrap().len(), 1);
        assert_eq!(
            s.pending_notifications(other, 10).unwrap().len(),
            1,
            "each folder is asked separately — the scheduler only asks about the inbox"
        );

        // Read it in the app before the sweep gets to it: it isn't news any more, and no notification
        // should arrive for a message the user is already looking at.
        s.set_seen(id, true).unwrap();
        assert!(s.pending_notifications(inbox, 10).unwrap().is_empty());
    }

    #[test]
    fn a_localized_special_folder_sorts_with_the_specials_not_among_the_users_own() {
        // Without the role, `Papierkorb` matches no name we know: it ranks as an ordinary folder and
        // sits under P, halfway down the rail, among the user's own. The order also decides which
        // folder wins if two ever carry the same role, so it isn't only cosmetic.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        for (name, role) in [
            ("Arbeit", None),
            ("Papierkorb", Some("trash")),
            ("Gesendet", Some("sent")),
            ("INBOX", Some("inbox")),
            ("Entwürfe", Some("drafts")),
            ("Archiv", Some("archive")),
        ] {
            s.upsert_folder_with_role(acc, name, role).unwrap();
        }
        let names: Vec<String> = s
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(
            names,
            [
                "INBOX",
                "Entwürfe",
                "Gesendet",
                "Archiv",
                "Papierkorb",
                "Arbeit"
            ],
            "specials in role order, the user's own last"
        );
    }

    #[test]
    fn upserting_a_folder_with_a_role_returns_the_folders_own_id() {
        // The id is what the caller syncs mail into. A listing re-runs this for every folder on every
        // sync, so returning the *same* row (not a new one) is the difference between a folder and a
        // duplicate of it.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let id = s
            .upsert_folder_with_role(acc, "Entwürfe", Some("drafts"))
            .unwrap();
        assert_eq!(
            s.folders_for_account(acc).unwrap()[0].id,
            id,
            "the id returned must be the folder's"
        );
        // Idempotent: listing again is the same row, and the role can change on it.
        let again = s.upsert_folder_with_role(acc, "Entwürfe", None).unwrap();
        assert_eq!(again, id);
        assert_eq!(s.folders_for_account(acc).unwrap().len(), 1);
        assert_eq!(s.folders_for_account(acc).unwrap()[0].role, None);
        // …and it agrees with the name-only upsert every message sync uses.
        assert_eq!(s.upsert_folder(acc, "Entwürfe").unwrap(), id);
    }

    #[test]
    fn a_new_draft_never_inherits_a_deleted_drafts_message_id() {
        // SQLite reuses the row id of the highest deleted row, so `draft.id` is NOT a stable identity.
        // A draft's Message-ID is what its copy on the server is stamped with — and what a re-save
        // expunges by. If a new draft could inherit a dead one's Message-ID, saving it would destroy
        // the dead draft's stranded copy on the server (and hide it from the Drafts list first).
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let c = DraftContent::default();

        let first = s.save_draft(acc, None, &c).unwrap();
        let doomed = s.save_draft(acc, None, &c).unwrap();
        let doomed_id = s.draft_by_id(doomed).unwrap().unwrap().msgid;
        s.delete_draft(doomed).unwrap();

        let reborn = s.save_draft(acc, None, &c).unwrap();
        let reborn_id = s.draft_by_id(reborn).unwrap().unwrap().msgid;
        assert_eq!(
            reborn, doomed,
            "SQLite really does hand the id back — that's the hazard"
        );
        assert_ne!(
            reborn_id, doomed_id,
            "…but the identity that reaches the server must not come back with it"
        );

        // And the ordinary guarantees: every draft's id is distinct, stamped, and stable across saves.
        let first_id = s.draft_by_id(first).unwrap().unwrap().msgid;
        assert_ne!(first_id, reborn_id);
        assert!(reborn_id.starts_with("<geleit-draft-") && reborn_id.ends_with("@geleit.local>"));
        s.save_draft(acc, Some(reborn), &c).unwrap();
        assert_eq!(
            s.draft_by_id(reborn).unwrap().unwrap().msgid,
            reborn_id,
            "a re-save keeps the id, or it could never find the copy it left last time"
        );
    }

    #[test]
    fn deleting_a_message_by_its_message_id_is_scoped_to_the_account() {
        // The mirrored copy of a deleted draft has to go with it, or the draft comes back as an "On
        // your provider" row. Two accounts on one server can hold the same Message-ID, so scope it.
        let s = Store::open_in_memory().unwrap();
        let (a, b) = (
            s.add_account("a@example.com", None).unwrap(),
            s.add_account("b@example.com", None).unwrap(),
        );
        let mid = "<geleit-draft-1-7-aa@geleit.local>";
        for acc in [a, b] {
            let f = s.upsert_folder(acc, "Drafts").unwrap();
            s.upsert_message(
                acc,
                f,
                &NewMessage {
                    uid: Some(1),
                    message_id: Some(mid.to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        assert_eq!(s.delete_message_by_message_id(a, mid).unwrap(), 1);
        assert_eq!(
            s.delete_message_by_message_id(a, mid).unwrap(),
            0,
            "idempotent — a copy we never held is not an error"
        );
        let b_drafts = s.folders_for_account(b).unwrap()[0].id;
        assert_eq!(
            s.messages_in_folder(b_drafts, 10).unwrap().len(),
            1,
            "the other account's message is untouched"
        );
    }

    #[test]
    fn drafts_in_folder_reads_the_whole_row_and_says_which_ones_are_formatted() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let drafts = s.upsert_folder(acc, "Drafts").unwrap();
        let other = s.upsert_folder(acc, "INBOX").unwrap();

        // A draft written in webmail (HTML), a plain one, and one in another folder entirely.
        let html = s
            .upsert_message(
                acc,
                drafts,
                &NewMessage {
                    uid: Some(1),
                    message_id: Some("<a@webmail>".to_owned()),
                    to_addrs: Some("hazel@example.org".to_owned()),
                    cc_addrs: Some("sam@example.org".to_owned()),
                    subject: Some("The roof".to_owned()),
                    date: Some(100),
                    ..Default::default()
                },
            )
            .unwrap();
        s.store_body(
            html,
            Some("the words"),
            Some("<p>the words</p>"),
            Some("the words"),
            false,
        )
        .unwrap();
        let plain = s
            .upsert_message(
                acc,
                drafts,
                &NewMessage {
                    uid: Some(2),
                    subject: Some("Plain".to_owned()),
                    date: Some(300),
                    ..Default::default()
                },
            )
            .unwrap();
        s.store_body(plain, Some("just text"), None, Some("just text"), false)
            .unwrap();
        s.upsert_message(
            acc,
            other,
            &NewMessage {
                uid: Some(3),
                subject: Some("not a draft".to_owned()),
                date: Some(400),
                ..Default::default()
            },
        )
        .unwrap();

        let rows = s.drafts_in_folder(drafts, 50).unwrap();
        assert_eq!(rows.len(), 2, "only this folder's drafts");
        // Newest first.
        assert_eq!(rows[0].id, plain);
        assert_eq!(rows[0].subject.as_deref(), Some("Plain"));
        assert!(
            !rows[0].formatted,
            "a plain-text draft loses nothing in our composer"
        );

        let h = &rows[1];
        assert_eq!(h.id, html);
        assert!(
            h.formatted,
            "it has an HTML body — continuing it would drop the styling, so the UI must ask"
        );
        // Everything the list needs, from the one query: who it's to, what it says, when, and whether
        // it's a copy we uploaded ourselves.
        assert_eq!(h.message_id.as_deref(), Some("<a@webmail>"));
        assert_eq!(h.to_addrs.as_deref(), Some("hazel@example.org"));
        assert_eq!(h.cc_addrs.as_deref(), Some("sam@example.org"));
        assert_eq!(h.snippet.as_deref(), Some("the words"));
        assert_eq!(h.date, Some(100));

        // The cap is honoured (a broken client could have appended thousands).
        assert_eq!(s.drafts_in_folder(drafts, 1).unwrap().len(), 1);
    }

    #[test]
    fn a_draft_with_no_body_yet_is_listed_and_is_not_called_formatted() {
        // It hasn't finished downloading. It must still show up (it's a draft on the provider), and it
        // must not be flagged as formatted — that would raise a warning about styling we've never seen.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let drafts = s.upsert_folder(acc, "Drafts").unwrap();
        s.upsert_message(
            acc,
            drafts,
            &NewMessage {
                uid: Some(1),
                subject: Some("No body yet".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        let rows = s.drafts_in_folder(drafts, 50).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].formatted);
    }

    #[test]
    fn messages_in_folder_newest_first_and_scoped() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let sent = s.upsert_folder(acc, "Sent").unwrap();
        for (uid, date, subj) in [(1, 100, "old"), (2, 300, "new"), (3, 200, "mid")] {
            s.upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(uid),
                    date: Some(date),
                    subject: Some(subj.to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        s.upsert_message(
            acc,
            sent,
            &NewMessage {
                uid: Some(9),
                subject: Some("elsewhere".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        let subs: Vec<_> = s
            .messages_in_folder(inbox, 50)
            .unwrap()
            .into_iter()
            .map(|m| m.subject.unwrap())
            .collect();
        assert_eq!(subs, ["new", "mid", "old"]); // date DESC
        assert_eq!(s.messages_in_folder(sent, 50).unwrap().len(), 1); // folder-scoped
    }

    #[test]
    fn folder_unread_counts_tallies_unseen_per_folder_and_omits_zero() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let other = s.add_account("b@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let archive = s.upsert_folder(acc, "Archive").unwrap();
        let sent = s.upsert_folder(acc, "Sent").unwrap();
        let mut uid = 0;
        let mut add = |folder: i64, seen: bool| {
            uid += 1;
            s.upsert_message(
                acc,
                folder,
                &NewMessage {
                    uid: Some(uid),
                    seen,
                    ..Default::default()
                },
            )
            .unwrap();
        };
        add(inbox, false); // inbox: 2 unread + 1 read
        add(inbox, false);
        add(inbox, true);
        add(archive, false); // archive: 1 unread
        add(sent, true); // sent: only read → must NOT appear

        // a message for the OTHER account must not leak into this account's tally
        s.upsert_folder(other, "INBOX").unwrap();
        let other_inbox = s.folders_for_account(other).unwrap()[0].id;
        s.upsert_message(
            other,
            other_inbox,
            &NewMessage {
                uid: Some(1),
                seen: false,
                ..Default::default()
            },
        )
        .unwrap();

        let mut got = s.folder_unread_counts(acc).unwrap();
        got.sort();
        let mut want = vec![(inbox, 2), (archive, 1)];
        want.sort();
        assert_eq!(got, want);

        // The window badge sums INBOX unread across BOTH accounts and nothing else: the archive unread
        // and everything in Sent are excluded, this account's inbox has 2 and the other's has 1.
        assert_eq!(
            s.total_inbox_unread().unwrap(),
            3,
            "2 in this account's inbox + 1 in the other's — archive and sent excluded"
        );
    }

    #[test]
    fn the_badge_counts_only_the_real_inbox_whatever_its_case_and_never_a_namespaced_lookalike() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.total_inbox_unread().unwrap(), 0, "nothing yet");
        let acc = s.add_account("a@example.com", None).unwrap();
        // A server that lower-cases the name is still the inbox (IMAP reserves it case-insensitively).
        let inbox = s.upsert_folder(acc, "inbox").unwrap();
        // These are NOT the inbox: a namespaced child (`INBOX.Archive`, the dovecot layout this project
        // develops against) and a folder that merely contains the word.
        let namespaced = s.upsert_folder(acc, "INBOX.Archive").unwrap();
        let lookalike = s.upsert_folder(acc, "Inbox archive").unwrap();
        let mut uid = 0;
        let mut add = |folder: i64, seen: bool| {
            uid += 1;
            s.upsert_message(
                acc,
                folder,
                &NewMessage {
                    uid: Some(uid),
                    seen,
                    ..Default::default()
                },
            )
            .unwrap();
        };
        add(inbox, false);
        add(inbox, false);
        add(inbox, true); // read → not counted
        add(namespaced, false); // INBOX.Archive → a different folder, not the inbox
        add(lookalike, false); // "Inbox archive" → not the inbox
        assert_eq!(s.total_inbox_unread().unwrap(), 2);
    }

    #[test]
    fn delete_folder_messages_clears_only_that_folder() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@x.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let trash = s.upsert_folder(acc, "Trash").unwrap();
        let mut uid = 0;
        let mut add = |folder: i64| {
            uid += 1;
            s.upsert_message(
                acc,
                folder,
                &NewMessage {
                    uid: Some(uid),
                    ..Default::default()
                },
            )
            .unwrap();
        };
        add(inbox);
        add(trash);
        add(trash);
        add(trash);

        assert_eq!(s.delete_folder_messages(trash).unwrap(), 3);
        assert_eq!(s.messages_in_folder(trash, 50).unwrap().len(), 0);
        assert_eq!(s.messages_in_folder(inbox, 50).unwrap().len(), 1); // untouched
    }

    #[test]
    fn rename_folder_keeps_the_id_so_messages_survive() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@x.com", None).unwrap();
        let work = s.upsert_folder(acc, "Work").unwrap();
        s.upsert_message(
            acc,
            work,
            &NewMessage {
                uid: Some(1),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(s.rename_folder(acc, "Work", "Projects").unwrap(), 1);
        // Same id, new name, message still attached.
        let folders = s.folders_for_account(acc).unwrap();
        let f = folders.iter().find(|f| f.id == work).expect("same row");
        assert_eq!(f.name, "Projects");
        assert_eq!(s.messages_in_folder(work, 50).unwrap().len(), 1);
        // Renaming a name that doesn't exist changes nothing.
        assert_eq!(s.rename_folder(acc, "Nope", "X").unwrap(), 0);
    }

    #[test]
    fn delete_folder_removes_the_row_and_cascades_its_messages() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@x.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let work = s.upsert_folder(acc, "Work").unwrap();
        for uid in 1..=2 {
            s.upsert_message(
                acc,
                work,
                &NewMessage {
                    uid: Some(uid),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        s.delete_folder(work).unwrap();
        let folders = s.folders_for_account(acc).unwrap();
        assert!(folders.iter().all(|f| f.id != work), "row gone");
        assert!(folders.iter().any(|f| f.id == inbox), "inbox kept");
        // The messages cascaded away with the folder.
        assert_eq!(s.messages_in_folder(work, 50).unwrap().len(), 0);
    }

    #[test]
    fn all_inboxes_merges_by_date_across_accounts_and_excludes_non_inbox() {
        let s = Store::open_in_memory().unwrap();
        let a = s.add_account("a@x.com", None).unwrap();
        let b = s.add_account("b@y.com", None).unwrap();
        let a_inbox = s.upsert_folder(a, "INBOX").unwrap();
        let a_sent = s.upsert_folder(a, "Sent").unwrap();
        let b_inbox = s.upsert_folder(b, "INBOX").unwrap();
        let mut uid = 0;
        let mut add = |acc: i64, folder: i64, date: i64| {
            uid += 1;
            s.upsert_message(
                acc,
                folder,
                &NewMessage {
                    uid: Some(uid),
                    date: Some(date),
                    ..Default::default()
                },
            )
            .unwrap();
        };
        add(a, a_inbox, 100);
        add(a, a_inbox, 300);
        add(a, a_sent, 500); // Sent — must NOT appear
        add(b, b_inbox, 200);

        let got: Vec<(Option<i64>, i64)> = s
            .messages_in_all_inboxes(50)
            .unwrap()
            .into_iter()
            .map(|(h, acc)| (h.date, acc))
            .collect();
        // newest first, merged across accounts, Sent excluded
        assert_eq!(got, [(Some(300), a), (Some(200), b), (Some(100), a)]);
    }

    #[test]
    fn snooze_hides_from_list_and_badge_then_resurfaces_and_re_owes_a_notification() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@x.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let mk = |uid: i64| {
            s.upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(uid),
                    date: Some(uid),
                    seen: false,
                    owed_notification: false, // already accounted for — resurfacing must re-owe it
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let keep = mk(1);
        let snoozed = mk(2);
        assert_eq!(s.messages_in_folder(inbox, 50).unwrap().len(), 2);
        assert_eq!(s.total_inbox_unread().unwrap(), 2);

        // Snooze one into the future: it leaves the list, the badge, and the per-folder count.
        s.snooze_messages(&[snoozed], now + 3600).unwrap();
        let listed: Vec<i64> = s
            .messages_in_folder(inbox, 50)
            .unwrap()
            .into_iter()
            .map(|h| h.id)
            .collect();
        assert_eq!(listed, vec![keep], "snoozed message is hidden");
        assert_eq!(s.total_inbox_unread().unwrap(), 1, "badge drops");
        assert_eq!(
            s.folder_unread_counts(acc).unwrap(),
            vec![(inbox, 1)],
            "folder count drops"
        );
        assert!(
            s.messages_in_all_inboxes(50)
                .unwrap()
                .iter()
                .all(|(h, _)| h.id != snoozed),
            "hidden from the unified inbox too"
        );

        // It shows in the Snoozed view, with its resurface time.
        let pane = s.snoozed_messages(acc).unwrap();
        assert_eq!(pane.len(), 1);
        assert_eq!(pane[0].id, snoozed);
        assert_eq!(pane[0].snoozed_until, now + 3600);

        // Not yet due → resurface is a no-op.
        assert_eq!(s.resurface_due_snoozes().unwrap(), 0);
        assert_eq!(s.messages_in_folder(inbox, 50).unwrap().len(), 1);

        // Make it due, then resurface: it returns to the list and is owed a notification again.
        s.snooze_messages(&[snoozed], now - 10).unwrap();
        assert_eq!(
            s.messages_in_folder(inbox, 50).unwrap().len(),
            2,
            "a due-but-uncleared snooze already shows (clock-based exclusion)"
        );
        assert_eq!(s.resurface_due_snoozes().unwrap(), 1);
        assert!(s.snoozed_messages(acc).unwrap().is_empty(), "cleared");
        let owed: Vec<i64> = s
            .pending_notifications(inbox, 50)
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert!(
            owed.contains(&snoozed),
            "resurfaced message is re-owed a notification"
        );

        // A message snoozed while READ resurfaces WITHOUT being re-owed a notification — so marking it
        // unread later can't pop a "new mail" alert for old mail.
        let read = mk(3);
        s.set_seen(read, true).unwrap();
        s.snooze_messages(&[read], now - 10).unwrap();
        assert_eq!(s.resurface_due_snoozes().unwrap(), 1);
        assert!(
            !s.pending_notifications(inbox, 50)
                .unwrap()
                .iter()
                .any(|p| p.id == read),
            "a read-then-snoozed message is not re-owed a notification"
        );

        // Un-snooze brings one back immediately. (Three messages are visible now: keep, the resurfaced
        // `snoozed`, and the resurfaced `read`.)
        s.snooze_messages(&[keep], now + 3600).unwrap();
        assert_eq!(
            s.messages_in_folder(inbox, 50).unwrap().len(),
            2,
            "keep hidden"
        );
        s.unsnooze_message(keep).unwrap();
        assert_eq!(
            s.messages_in_folder(inbox, 50).unwrap().len(),
            3,
            "keep back"
        );
    }

    #[test]
    fn rules_crud_and_the_filtered_marker() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@x.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();

        // Rules keep insertion order (first-match-wins) and round-trip their actions.
        let r1 = s
            .add_rule(acc, "from", "newsletter", Some("Reading"), true, false, 10)
            .unwrap();
        let r2 = s
            .add_rule(acc, "subject", "invoice", None, false, true, 20)
            .unwrap();
        let rules = s.list_rules(acc).unwrap();
        assert_eq!(rules.iter().map(|r| r.id).collect::<Vec<_>>(), vec![r1, r2]);
        assert_eq!(rules[0].field, "from");
        assert_eq!(rules[0].target_folder.as_deref(), Some("Reading"));
        assert!(rules[0].mark_read && !rules[0].star);
        assert_eq!(rules[1].target_folder, None);
        assert!(!rules[1].mark_read && rules[1].star);

        // Reorder: moving r2 up swaps the evaluation order (first-match-wins priority).
        let order = |s: &Store| {
            s.list_rules(acc)
                .unwrap()
                .into_iter()
                .map(|r| r.id)
                .collect::<Vec<_>>()
        };
        s.move_rule(r2, true).unwrap();
        assert_eq!(order(&s), vec![r2, r1], "r2 moved above r1");
        s.move_rule(r2, true).unwrap(); // already at the top — no-op
        assert_eq!(order(&s), vec![r2, r1]);
        s.move_rule(r2, false).unwrap();
        assert_eq!(order(&s), vec![r1, r2], "r2 back below r1");
        s.move_rule(999, true).unwrap(); // unknown id — no-op, no panic

        // A genuinely-new message is owed a rule pass; a backfilled one is not.
        let fresh = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(1),
                    from_addr: Some("news@x.com".into()),
                    subject: Some("Hi".into()),
                    owed_notification: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let old = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(2),
                    owed_notification: false,
                    ..Default::default()
                },
            )
            .unwrap();
        let unfiltered: Vec<i64> = s
            .unfiltered_inbox(acc)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(
            unfiltered,
            vec![fresh],
            "only the new message is owed a pass"
        );

        // Marking it filtered removes it from the queue.
        s.mark_filtered(fresh).unwrap();
        assert!(s.unfiltered_inbox(acc).unwrap().is_empty());

        // "Run on inbox now" re-arms the whole inbox (both messages).
        assert_eq!(s.reset_inbox_filtered(acc).unwrap(), 2);
        let requeued: Vec<i64> = s
            .unfiltered_inbox(acc)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(requeued, vec![fresh, old]);

        // Deleting a rule; the account's rules cascade when the account goes.
        s.delete_rule(r1).unwrap();
        assert_eq!(s.list_rules(acc).unwrap().len(), 1);
        s.delete_account(acc).unwrap();
        // (a fresh account id would have no rules — the FK cascade cleared them)
    }

    #[test]
    fn folder_message_ids_are_chronological_and_include_snoozed() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@x.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let other = s.upsert_folder(acc, "Work").unwrap();
        let mk = |folder: i64, uid: i64, date: i64| {
            s.upsert_message(
                acc,
                folder,
                &NewMessage {
                    uid: Some(uid),
                    date: Some(date),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let newest = mk(inbox, 1, 300);
        let oldest = mk(inbox, 2, 100);
        let mid = mk(inbox, 3, 200);
        mk(other, 4, 250); // a different folder — must not appear

        // Oldest-first, this folder only.
        assert_eq!(
            s.folder_message_ids(inbox).unwrap(),
            vec![oldest, mid, newest]
        );

        // A snoozed message is still in the export (a backup leaves nothing out), unlike the listing.
        s.snooze_messages(&[mid], now + 3600).unwrap();
        assert_eq!(
            s.messages_in_folder(inbox, 50).unwrap().len(),
            2,
            "the listing hides it"
        );
        assert_eq!(
            s.folder_message_ids(inbox).unwrap(),
            vec![oldest, mid, newest],
            "the export keeps it"
        );
    }

    #[test]
    fn draft_attachments_roundtrip_replace_and_cascade() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let id = s.save_draft(acc, None, &DraftContent::default()).unwrap();
        assert!(s.draft_attachments(id).unwrap().is_empty());

        let a1 = DraftAttachment {
            filename: Some("a.pdf".into()),
            content_type: "application/pdf".into(),
            data: vec![1, 2, 3],
        };
        let a2 = DraftAttachment {
            filename: None,
            content_type: "text/plain".into(),
            data: vec![9, 9],
        };
        s.replace_draft_attachments(id, &[a1.clone(), a2.clone()])
            .unwrap();
        assert_eq!(s.draft_attachments(id).unwrap(), vec![a1.clone(), a2]); // order + bytes preserved

        // replace overwrites the whole set (not append)
        s.replace_draft_attachments(id, std::slice::from_ref(&a1))
            .unwrap();
        assert_eq!(s.draft_attachments(id).unwrap(), vec![a1]);
        // clearing
        s.replace_draft_attachments(id, &[]).unwrap();
        assert!(s.draft_attachments(id).unwrap().is_empty());

        // cascade: deleting the draft removes its attachments
        s.replace_draft_attachments(
            id,
            &[DraftAttachment {
                filename: Some("x".into()),
                content_type: "application/octet-stream".into(),
                data: vec![0],
            }],
        )
        .unwrap();
        s.delete_draft(id).unwrap();
        assert!(s.draft_attachments(id).unwrap().is_empty());
    }

    #[test]
    fn draft_server_folder_is_recorded_cleared_and_survives_a_resave() {
        let s = Store::open_in_memory().unwrap();
        // Two accounts, and the draft on the *second* — so `account_for_draft` can't pass by
        // returning a hard-coded first id.
        let other = s.add_account("first@example.com", None).unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let c = DraftContent {
            subject: "Hi".into(),
            ..Default::default()
        };
        let id = s.save_draft(acc, None, &c).unwrap();
        // A fresh draft is local-only, and knows which account it belongs to.
        assert_eq!(s.draft_by_id(id).unwrap().unwrap().server_folder, None);
        assert_eq!(s.account_for_draft(id).unwrap(), Some(acc));
        assert_eq!(s.account_for_draft(9_999).unwrap(), None); // no such draft

        s.set_draft_server_folder(id, Some("Drafts")).unwrap();
        assert_eq!(
            s.draft_by_id(id).unwrap().unwrap().server_folder.as_deref(),
            Some("Drafts")
        );
        // Re-saving the content must not clobber the recorded server folder.
        let c2 = DraftContent {
            subject: "Hi again".into(),
            ..Default::default()
        };
        s.save_draft(acc, Some(id), &c2).unwrap();
        let row = s.draft_by_id(id).unwrap().unwrap();
        assert_eq!(row.content.subject, "Hi again");
        assert_eq!(row.server_folder.as_deref(), Some("Drafts"));
        // It also shows up on the list rows, and clearing works.
        assert_eq!(
            s.list_drafts(acc).unwrap()[0].server_folder,
            row.server_folder
        );

        // The sweep list (used when "sync drafts" is switched off) names exactly the drafts that have
        // a copy on the server — not the local-only ones, and not another account's.
        let local_only = s.save_draft(acc, None, &c).unwrap();
        let others = s.save_draft(other, None, &c).unwrap();
        s.set_draft_server_folder(others, Some("INBOX.Drafts"))
            .unwrap();
        // Each row carries the draft's own stored Message-ID — the copy on the server is expunged by
        // it, so it must be the one that draft was appended under, never one re-derived from its id.
        let mine = s.draft_by_id(id).unwrap().unwrap().msgid;
        assert_eq!(
            s.drafts_with_server_copies(acc).unwrap(),
            vec![(id, "Drafts".to_owned(), mine)],
            "only this account's synced draft"
        );
        assert_eq!(
            s.drafts_with_server_copies(other).unwrap(),
            vec![(
                others,
                "INBOX.Drafts".to_owned(),
                s.draft_by_id(others).unwrap().unwrap().msgid
            )]
        );
        assert!(s
            .draft_by_id(local_only)
            .unwrap()
            .unwrap()
            .server_folder
            .is_none());

        s.set_draft_server_folder(id, None).unwrap();
        assert_eq!(s.draft_by_id(id).unwrap().unwrap().server_folder, None);
        // Cleared → it drops off the sweep list.
        assert!(s.drafts_with_server_copies(acc).unwrap().is_empty());
    }

    #[test]
    fn draft_save_list_resume_update_delete() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        assert!(s.list_drafts(acc).unwrap().is_empty());

        let c = DraftContent {
            to: "bob@x.com".into(),
            cc: "carol@x.com".into(),
            subject: "Hi".into(),
            body: "draft body".into(),
            in_reply_to: Some("<m1@x>".into()),
            references: vec!["<m0@x>".into(), "<m1@x>".into()],
        };
        let id = s.save_draft(acc, None, &c).unwrap();
        let row = s.draft_by_id(id).unwrap().expect("found");
        assert_eq!(row.content, c); // full round-trip incl. references split/join
        assert_eq!(s.list_drafts(acc).unwrap().len(), 1);

        // update in place (same id, no new row)
        let mut c2 = c.clone();
        c2.subject = "Hi again".into();
        c2.references = vec![]; // clearing references → None in DB → empty Vec back
        let id2 = s.save_draft(acc, Some(id), &c2).unwrap();
        assert_eq!(id2, id);
        let row = s.draft_by_id(id).unwrap().unwrap();
        assert_eq!(row.content.subject, "Hi again");
        assert!(row.content.references.is_empty());
        assert_eq!(s.list_drafts(acc).unwrap().len(), 1);

        s.delete_draft(id).unwrap();
        assert!(s.draft_by_id(id).unwrap().is_none());
        assert!(s.list_drafts(acc).unwrap().is_empty());
    }

    #[test]
    fn signature_roundtrip_and_clear() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        assert_eq!(s.signature(acc).unwrap(), None);
        s.update_signature(acc, "— Alice\nSent from GeleitMail")
            .unwrap();
        assert_eq!(
            s.signature(acc).unwrap().as_deref(),
            Some("— Alice\nSent from GeleitMail")
        );
        s.update_signature(acc, "").unwrap(); // empty clears it
        assert_eq!(s.signature(acc).unwrap(), None);
    }

    #[test]
    fn parse_search_quotes_terms_operators_and_filters() {
        use super::parse_search;
        let mq = |s: &str| parse_search(s).match_query;
        assert_eq!(mq(""), None);
        assert_eq!(mq("   "), None);
        assert_eq!(mq("!!! ??"), None); // no alphanumerics
        assert_eq!(mq("hello").as_deref(), Some("\"hello\"*"));
        assert_eq!(mq("foo bar").as_deref(), Some("\"foo\" \"bar\"*"));
        // an embedded quote is doubled, so it can't break out of the phrase
        assert_eq!(mq("a\"b").as_deref(), Some("\"a\"\"b\"*"));
        // operators: from:/subject: scope to a column (case-insensitive); has:attachment is a filter
        assert_eq!(
            parse_search("From:alice budget").match_query.as_deref(),
            Some("sender:\"alice\" \"budget\"*")
        );
        assert_eq!(
            parse_search("subject:Q3").match_query.as_deref(),
            Some("subject:\"Q3\"*")
        );
        let p = parse_search("has:attachment invoice");
        assert!(p.require_attachment);
        assert_eq!(p.match_query.as_deref(), Some("\"invoice\"*"));
        // a bare has:attachments with no terms → filter only, no MATCH
        let only = parse_search("has:attachments");
        assert!(only.require_attachment);
        assert_eq!(only.match_query, None);
        // an empty operator value contributes nothing
        assert_eq!(parse_search("from:").match_query, None);
    }

    #[test]
    fn search_indexes_subject_sender_body_and_unindexes_on_delete() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("me@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let id = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(1),
                    subject: Some("Quarterly invoice".to_owned()),
                    from_name: Some("Alice Baker".to_owned()),
                    from_addr: Some("alice@vendor.test".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        // subject + sender are searchable right after the envelope upsert
        assert_eq!(s.search_messages(acc, "invoice", 10).unwrap().len(), 1);
        assert_eq!(s.search_messages(acc, "alice", 10).unwrap()[0].id, id);
        assert_eq!(s.search_messages(acc, "baker", 10).unwrap().len(), 1);
        // body is searchable once stored
        assert!(s.search_messages(acc, "umbrella", 10).unwrap().is_empty());
        s.store_body(
            id,
            Some("please find the umbrella attached"),
            None,
            Some("…"),
            false,
        )
        .unwrap();
        let hit = s.search_messages(acc, "umbrella", 10).unwrap();
        assert_eq!(hit.len(), 1);
        // the result's snippet is the match context (SEARCH highlighting), not the stored preview "…"
        let snip = hit[0].snippet.as_deref().unwrap_or_default();
        assert!(
            snip.contains("umbrella"),
            "match-context snippet, got {snip:?}"
        );
        // prefix (type-ahead) matches; another account doesn't
        assert_eq!(s.search_messages(acc, "umbr", 10).unwrap().len(), 1);
        let other = s.add_account("other@example.com", None).unwrap();
        assert!(s.search_messages(other, "invoice", 10).unwrap().is_empty());
        // empty query → no rows; deleting drops it from the index (via trigger)
        assert!(s.search_messages(acc, "  ", 10).unwrap().is_empty());
        s.delete_message(id).unwrap();
        assert!(s.search_messages(acc, "invoice", 10).unwrap().is_empty());
    }

    #[test]
    fn search_all_accounts_spans_accounts_with_ids() {
        let s = Store::open_in_memory().unwrap();
        let a = s.add_account("a@example.com", None).unwrap();
        let b = s.add_account("b@example.com", None).unwrap();
        let ia = s.upsert_folder(a, "INBOX").unwrap();
        let ib = s.upsert_folder(b, "INBOX").unwrap();
        let ma = s
            .upsert_message(
                a,
                ia,
                &NewMessage {
                    uid: Some(1),
                    subject: Some("shared keyword alpha".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        // a's message gets a body word + an attachment; b's does not
        s.store_body(ma, Some("the rendezvous point is set"), None, None, true)
            .unwrap();
        s.upsert_message(
            b,
            ib,
            &NewMessage {
                uid: Some(1),
                subject: Some("shared keyword beta".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        // per-account search sees only its own; all-accounts sees both, tagged with the account id
        assert_eq!(s.search_messages(a, "shared", 10).unwrap().len(), 1);
        let all = s.search_all_accounts("shared", 10).unwrap();
        assert_eq!(all.len(), 2);
        let mut accts: Vec<i64> = all.iter().map(|(_, acc)| *acc).collect();
        accts.sort_unstable();
        assert_eq!(accts, vec![a, b]);
        // a body match is found across accounts AND its snippet shows the match context (kills the
        // `if !snip.is_empty()` mutant)
        let body_hit = s.search_all_accounts("rendezvous", 10).unwrap();
        assert_eq!(body_hit.len(), 1);
        assert_eq!(body_hit[0].1, a);
        assert!(body_hit[0]
            .0
            .snippet
            .as_deref()
            .unwrap_or_default()
            .contains("rendezvous"));
        // has:attachment across accounts → only a (FTS+filter, and filter-only) — kills guard→false
        assert_eq!(
            s.search_all_accounts("shared has:attachment", 10)
                .unwrap()
                .len(),
            1
        );
        let only_att = s.search_all_accounts("has:attachment", 10).unwrap();
        assert_eq!(only_att.len(), 1);
        assert_eq!(only_att[0].1, a);
        // empty query → nothing even though an attachment-bearing message exists — kills guard→true
        assert!(s.search_all_accounts("  ", 10).unwrap().is_empty());
    }

    #[test]
    fn search_operators_scope_and_filter() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("me@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        // m1: from Alice, subject mentions "report", no attachment
        s.upsert_message(
            acc,
            inbox,
            &NewMessage {
                uid: Some(1),
                subject: Some("weekly report".to_owned()),
                from_name: Some("Alice".to_owned()),
                from_addr: Some("alice@x.test".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        // m2: from Bob, body mentions "alice", WITH attachment
        let m2 = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(2),
                    subject: Some("lunch".to_owned()),
                    from_name: Some("Bob".to_owned()),
                    from_addr: Some("bob@x.test".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        s.store_body(m2, Some("tell alice the report is late"), None, None, true)
            .unwrap();

        // from:alice scopes to the sender column → only m1 (Bob's body mentions alice, but not sender)
        let from_alice = s.search_messages(acc, "from:alice", 10).unwrap();
        assert_eq!(from_alice.len(), 1);
        assert_eq!(from_alice[0].uid, Some(1));
        // subject:report → only m1
        assert_eq!(
            s.search_messages(acc, "subject:report", 10).unwrap().len(),
            1
        );
        // bare "alice" hits both (m1 sender, m2 body)
        assert_eq!(s.search_messages(acc, "alice", 10).unwrap().len(), 2);
        // has:attachment filters: "report has:attachment" → only m2 (body has report + attachment)
        let with_att = s.search_messages(acc, "report has:attachment", 10).unwrap();
        assert_eq!(with_att.len(), 1);
        assert_eq!(with_att[0].uid, Some(2));
        // filter-only (no full-text terms) lists messages with attachments
        let only_att = s.search_messages(acc, "has:attachment", 10).unwrap();
        assert_eq!(only_att.len(), 1);
        assert_eq!(only_att[0].uid, Some(2));
        // an empty query returns NOTHING even though an attachment-bearing message exists (the
        // attachment filter must be *required*, not assumed for every no-term query)
        assert!(s.search_messages(acc, "   ", 10).unwrap().is_empty());
    }

    #[test]
    fn backfill_builds_index_on_open_when_empty_but_messages_exist() {
        let path = std::env::temp_dir().join(format!("geleit-backfill-{}.db", std::process::id()));
        let path = path.to_str().unwrap();
        let _ = std::fs::remove_file(path);
        {
            let s = Store::open(path).unwrap();
            let acc = s.add_account("me@example.com", None).unwrap();
            let inbox = s.upsert_folder(acc, "INBOX").unwrap();
            s.upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(1),
                    subject: Some("backfillme".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
            // simulate data that predates migration #10: an empty index over existing messages
            s.conn.execute_batch("DELETE FROM message_fts").unwrap();
            assert!(s.search_messages(acc, "backfillme", 10).unwrap().is_empty());
        }
        // reopening runs init → backfill_search_index, which rebuilds the index
        {
            let s = Store::open(path).unwrap();
            let acc = s.list_accounts().unwrap()[0].id;
            assert_eq!(s.search_messages(acc, "backfillme", 10).unwrap().len(), 1);
            // a second open must NOT wipe/rebuild needlessly — index already populated, still works
            drop(s);
            let s = Store::open(path).unwrap();
            assert_eq!(s.search_messages(acc, "backfillme", 10).unwrap().len(), 1);
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reindex_all_rebuilds_after_direct_insert() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("me@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        s.upsert_message(
            acc,
            inbox,
            &NewMessage {
                uid: Some(7),
                subject: Some("Reindexable".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        // wipe the index, then rebuild it
        s.conn.execute_batch("DELETE FROM message_fts").unwrap();
        assert!(s
            .search_messages(acc, "reindexable", 10)
            .unwrap()
            .is_empty());
        assert_eq!(s.reindex_all().unwrap(), 1);
        assert_eq!(s.search_messages(acc, "reindexable", 10).unwrap().len(), 1);
    }

    #[test]
    fn settings_get_set_upsert() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.get_setting("theme").unwrap(), None);
        s.set_setting("theme", "dark").unwrap();
        assert_eq!(s.get_setting("theme").unwrap().as_deref(), Some("dark"));
        s.set_setting("theme", "light").unwrap(); // upsert replaces
        assert_eq!(s.get_setting("theme").unwrap().as_deref(), Some("light"));
        assert_eq!(s.get_setting("absent").unwrap(), None);
    }

    #[test]
    fn account_by_id_and_isolation_across_accounts() {
        let s = Store::open_in_memory().unwrap();
        let a = s.add_account("a@example.com", Some("Ann")).unwrap();
        let b = s.add_account("b@example.com", None).unwrap();
        assert_eq!(s.account_by_id(a).unwrap().unwrap().email, "a@example.com");
        assert_eq!(s.account_by_id(b).unwrap().unwrap().display_name, None);
        assert!(s.account_by_id(9999).unwrap().is_none());
        // folders/messages are per-account: indexing/listing one doesn't bleed into the other
        let inbox_a = s.upsert_folder(a, "INBOX").unwrap();
        s.upsert_message(
            a,
            inbox_a,
            &NewMessage {
                uid: Some(1),
                subject: Some("hi from a".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(s.search_messages(a, "hi", 10).unwrap().len(), 1);
        assert!(s.search_messages(b, "hi", 10).unwrap().is_empty());
        assert!(s.folders_for_account(b).unwrap().is_empty());
    }

    #[test]
    fn folders_ordered_inbox_first_then_specials_then_alpha() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        // insert in a deliberately jumbled order, including provider variants
        for f in [
            "Zebra",
            "Deleted Items",
            "INBOX",
            "Archive",
            "Work",
            "Sent Mail",
            "Junk Email",
            "Drafts",
            "apple",
        ] {
            s.upsert_folder(acc, f).unwrap();
        }
        let names: Vec<String> = s
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(
            names,
            [
                "INBOX",         // 0
                "Drafts",        // 1
                "Sent Mail",     // 2
                "Archive",       // 3
                "Junk Email",    // 4 (contains "junk")
                "Deleted Items", // 5 (contains "deleted")
                "apple",         // 6, then alphabetical (case-insensitive)
                "Work",
                "Zebra",
            ]
        );
    }

    #[test]
    fn prune_folders_removes_absent_keeps_listed() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        for n in ["INBOX", "Sent", "Old"] {
            s.upsert_folder(acc, n).unwrap();
        }
        s.prune_folders(acc, &["INBOX".to_owned(), "Sent".to_owned()])
            .unwrap();
        let names: Vec<_> = s
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, ["INBOX", "Sent"]); // "Old" pruned
    }

    #[test]
    fn prune_folders_keeps_local_saved_folder() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        for n in ["INBOX", SAVED_FOLDER, "Old"] {
            s.upsert_folder(acc, n).unwrap();
        }
        // the server's folder list (keep) never includes "Saved", yet it must survive
        s.prune_folders(acc, &["INBOX".to_owned()]).unwrap();
        let names: Vec<_> = s
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert!(names.contains(&SAVED_FOLDER.to_owned())); // local folder kept
        assert!(!names.contains(&"Old".to_owned())); // server-absent folder pruned
    }

    #[test]
    fn message_location_returns_folder_and_uid() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let id = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(42),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            s.message_location(id).unwrap(),
            Some(("INBOX".to_owned(), 42))
        );
        // local-only message (no uid) → None
        let local = s
            .upsert_message(acc, inbox, &NewMessage::default())
            .unwrap();
        assert_eq!(s.message_location(local).unwrap(), None);
        assert_eq!(s.message_location(9999).unwrap(), None);
    }

    #[test]
    fn delete_message_removes_the_row() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let id = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(1),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(s.messages_in_folder(inbox, 10).unwrap().len(), 1);
        s.delete_message(id).unwrap();
        assert!(s.header_by_id(id).unwrap().is_none());
        assert!(s.messages_in_folder(inbox, 10).unwrap().is_empty());
    }

    #[test]
    fn flagged_synced_on_insert_preserved_on_resync_and_settable() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        // first sync: server has it flagged
        let id = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(1),
                    flagged: true,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(s.header_by_id(id).unwrap().unwrap().flagged);
        // local unstar, then an envelope re-sync (server still says flagged) must NOT clobber it
        let uid = s.set_flagged(id, false).unwrap();
        assert_eq!(uid, Some(1));
        s.upsert_message(
            acc,
            inbox,
            &NewMessage {
                uid: Some(1),
                flagged: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!s.header_by_id(id).unwrap().unwrap().flagged); // local state preserved
                                                                // listing exposes the flag too
        assert!(!s.messages_in_folder(inbox, 10).unwrap()[0].flagged);
    }

    #[test]
    fn suggest_addresses_prefix_distinct_sorted() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        for (uid, addr) in [(1, "alice@x"), (2, "bob@y"), (3, "alan@z"), (4, "alice@x")] {
            s.upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(uid),
                    from_addr: Some(addr.to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        // prefix, case-insensitive, distinct, alphabetical
        assert_eq!(
            s.suggest_addresses(acc, "al", 10).unwrap(),
            ["alan@z", "alice@x"]
        );
        assert_eq!(
            s.suggest_addresses(acc, "AL", 10).unwrap(),
            ["alan@z", "alice@x"]
        );
        assert_eq!(s.suggest_addresses(acc, "bob", 10).unwrap(), ["bob@y"]);
        assert!(s.suggest_addresses(acc, "zzz", 10).unwrap().is_empty());
        assert!(s.suggest_addresses(acc, "  ", 10).unwrap().is_empty());
        // literal % is escaped (doesn't match-all)
        assert!(s.suggest_addresses(acc, "%", 10).unwrap().is_empty());
    }

    #[test]
    fn header_by_id_fetches_one_or_none() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let id = s
            .upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(1),
                    subject: Some("Hello".to_owned()),
                    from_addr: Some("bob@x.com".to_owned()),
                    message_id: Some("<m1@x>".to_owned()),
                    to_addrs: Some("me@x.com, carol@x.com".to_owned()),
                    cc_addrs: Some("dave@x.com".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        let h = s.header_by_id(id).unwrap().expect("found");
        assert_eq!(h.subject.as_deref(), Some("Hello"));
        assert_eq!(h.from_addr.as_deref(), Some("bob@x.com"));
        assert_eq!(h.message_id.as_deref(), Some("<m1@x>"));
        assert_eq!(h.to_addrs.as_deref(), Some("me@x.com, carol@x.com"));
        assert_eq!(h.cc_addrs.as_deref(), Some("dave@x.com"));
        assert_eq!(s.header_by_id(999_999).unwrap(), None);
    }

    #[test]
    fn store_body_writes_body_and_updates_message() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();
        let mid = s
            .upsert_message(
                acc,
                fld,
                &NewMessage {
                    uid: Some(7),
                    subject: Some("Hi".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(s.message_id_by_uid(acc, fld, 7).unwrap(), Some(mid));
        assert!(s.body_for(mid).unwrap().is_none());

        s.store_body(
            mid,
            Some("plain text"),
            Some("<p>html</p>"),
            Some("plain text"),
            true,
        )
        .unwrap();
        assert_eq!(
            s.body_for(mid).unwrap(),
            Some(StoredBody {
                plain: Some("plain text".to_owned()),
                html: Some("<p>html</p>".to_owned()),
            })
        );
        let hdr = &s.messages_in_folder(fld, 1).unwrap()[0];
        assert!(hdr.has_attachments);

        // re-store updates the same body row (no duplicate)
        s.store_body(mid, Some("v2"), None, Some("v2"), false)
            .unwrap();
        assert_eq!(
            s.body_for(mid).unwrap(),
            Some(StoredBody {
                plain: Some("v2".to_owned()),
                html: None,
            })
        );
        assert!(!s.messages_in_folder(fld, 1).unwrap()[0].has_attachments);
    }

    #[test]
    fn envelope_resync_preserves_body_derived_fields() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();
        let mid = s
            .upsert_message(
                acc,
                fld,
                &NewMessage {
                    uid: Some(1),
                    subject: Some("first".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        s.store_body(mid, Some("p"), None, Some("preview"), true)
            .unwrap();
        // envelope-only re-sync (no body fields) must not wipe snippet/has_attachments
        s.upsert_message(
            acc,
            fld,
            &NewMessage {
                uid: Some(1),
                subject: Some("updated".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        let h = &s.messages_in_folder(fld, 1).unwrap()[0];
        assert_eq!(h.subject.as_deref(), Some("updated")); // envelope field refreshed
        assert!(h.has_attachments); // body-derived: preserved
        assert_eq!(h.snippet.as_deref(), Some("preview")); // body-derived: preserved
    }

    #[test]
    fn store_body_for_unknown_message_fails_cleanly() {
        let s = Store::open_in_memory().unwrap();
        // FK violation on the body insert → the whole transaction rolls back, nothing committed.
        assert!(s.store_body(999, Some("x"), None, None, false).is_err());
        assert!(s.body_for(999).unwrap().is_none());
    }

    #[test]
    fn incremental_sync_store_methods() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();

        // uidvalidity round-trip
        assert_eq!(s.folder_uidvalidity(fld).unwrap(), None);
        s.set_folder_uidvalidity(fld, 42).unwrap();
        assert_eq!(s.folder_uidvalidity(fld).unwrap(), Some(42));

        for uid in [10, 11, 12] {
            s.upsert_message(
                acc,
                fld,
                &NewMessage {
                    uid: Some(uid),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let mut uids = s.uids_in_folder(fld).unwrap();
        uids.sort_unstable();
        assert_eq!(uids, vec![10, 11, 12]);

        // delete by uid (and a body cascades)
        let mid = s.message_id_by_uid(acc, fld, 11).unwrap().unwrap();
        s.store_body(mid, Some("b"), None, None, false).unwrap();
        s.delete_messages_by_uid(fld, &[11]).unwrap();
        let mut uids = s.uids_in_folder(fld).unwrap();
        uids.sort_unstable();
        assert_eq!(uids, vec![10, 12]);
        assert!(s.body_for(mid).unwrap().is_none()); // body cascaded
        s.delete_messages_by_uid(fld, &[]).unwrap(); // empty no-op

        // uids_without_body: messages 10 & 12 remain, neither has a body now
        let mut missing = s.uids_without_body(fld, 50).unwrap();
        missing.sort_unstable();
        assert_eq!(missing, vec![10, 12]);
        let mid10 = s.message_id_by_uid(acc, fld, 10).unwrap().unwrap();
        s.store_body(mid10, Some("b"), None, None, false).unwrap();
        assert_eq!(s.uids_without_body(fld, 50).unwrap(), vec![12]); // 10 now has a body

        // clear folder
        s.clear_folder(fld).unwrap();
        assert!(s.uids_in_folder(fld).unwrap().is_empty());
    }

    #[test]
    fn set_seen_flips_read_state() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();
        let mid = s
            .upsert_message(
                acc,
                fld,
                &NewMessage {
                    uid: Some(1),
                    seen: false,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(!s.messages_in_folder(fld, 1).unwrap()[0].seen);
        s.set_seen(mid, true).unwrap();
        assert!(s.messages_in_folder(fld, 1).unwrap()[0].seen);
        s.set_seen(mid, false).unwrap();
        assert!(!s.messages_in_folder(fld, 1).unwrap()[0].seen);
    }

    #[test]
    fn pulling_server_flag_changes_moves_only_seen_and_flagged_and_only_where_they_differ() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();
        let add = |uid: i64, seen: bool, flagged: bool| {
            s.upsert_message(
                acc,
                fld,
                &NewMessage {
                    uid: Some(uid),
                    subject: Some(format!("m{uid}")),
                    seen,
                    flagged,
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let m1 = add(1, false, false); // will be read elsewhere
        let m2 = add(2, true, true); // will be unstarred elsewhere
        let m3 = add(3, true, false); // untouched

        let mut flags = s.flags_in_folder(fld).unwrap();
        flags.sort();
        assert_eq!(
            flags,
            vec![(1, false, false), (2, true, true), (3, true, false)]
        );

        // Apply what the reconciliation computed: m1 now read, m2 now unstarred.
        s.apply_flag_changes(fld, &[(1, true, false), (2, true, false)])
            .unwrap();

        let seen_of = |id: i64| s.header_by_id(id).unwrap().unwrap().seen;
        let flagged_of = |id: i64| s.header_by_id(id).unwrap().unwrap().flagged;
        assert!(seen_of(m1), "read on another device → read here");
        assert!(!flagged_of(m2), "unstarred elsewhere → unstarred here");
        assert!(seen_of(m2), "…but still read");
        assert!(
            seen_of(m3) && !flagged_of(m3),
            "untouched message unchanged"
        );

        // The envelope is never touched by a flag pull.
        assert_eq!(
            s.header_by_id(m1).unwrap().unwrap().subject.as_deref(),
            Some("m1")
        );
        // An empty change set is a no-op, not an error.
        s.apply_flag_changes(fld, &[]).unwrap();
    }

    #[test]
    fn the_writeback_queue_holds_every_unconfirmed_flag_change_across_folders_and_accounts() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let other = s.add_account("b@example.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let archive = s.upsert_folder(acc, "Archive").unwrap();
        let add = |account: i64, folder: i64, uid: i64| {
            s.upsert_message(
                account,
                folder,
                &NewMessage {
                    uid: Some(uid),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let m_read = add(acc, inbox, 1); // will be read here
        let m_star = add(acc, archive, 2); // starred in another folder
        add(acc, inbox, 3); // untouched → not owed
        let m_other = add(other, inbox, 9); // a different account's change

        // Nothing owed until a local change is made.
        assert!(s.pending_flag_writebacks(acc).unwrap().is_empty());

        s.set_seen(m_read, true).unwrap();
        s.set_flagged(m_star, true).unwrap();
        s.set_seen(m_other, true).unwrap();

        let mut owed = s.pending_flag_writebacks(acc).unwrap();
        owed.sort_by_key(|p| p.uid);
        assert_eq!(
            owed.len(),
            2,
            "this account's two changes, across two folders — not the other's"
        );
        assert_eq!(owed[0].folder, "INBOX");
        assert_eq!(
            (owed[0].uid, owed[0].seen, owed[0].flagged),
            (1, true, false)
        );
        assert_eq!(owed[1].folder, "Archive");
        assert_eq!(
            (owed[1].uid, owed[1].seen, owed[1].flagged),
            (2, false, true),
            "starred but not read"
        );

        // The other account's queue holds only its own.
        assert_eq!(s.pending_flag_writebacks(other).unwrap().len(), 1);

        // Confirming one drains it from the queue; the rest stay owed until they land. The clear is
        // conditional on the pushed flags — passing the wrong ones would be a no-op.
        assert!(
            !s.clear_flags_dirty(m_read, false, false).unwrap(),
            "stale flags don't clear"
        );
        assert!(
            s.clear_flags_dirty(m_read, true, false).unwrap(),
            "the flags we pushed clear it"
        );
        let left = s.pending_flag_writebacks(acc).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, m_star);
    }

    #[test]
    fn the_outbox_queues_retries_and_surfaces_a_rejected_send() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let other = s.add_account("b@example.com", None).unwrap();
        assert_eq!(s.outbox_counts().unwrap(), (0, 0), "empty to start");

        let a = s
            .enqueue_outbox(
                acc,
                "a@example.com",
                &["x@y.io".to_owned(), "z@y.io".to_owned()],
                "Hi",
                b"raw-a",
                Some(("Drafts", "<d1@geleit.local>")), // a resumed provider draft
                100,
            )
            .unwrap();
        let b = s
            .enqueue_outbox(
                acc,
                "a@example.com",
                &["w@y.io".to_owned()],
                "Later",
                b"raw-b",
                None,
                200,
            )
            .unwrap();
        let other_c = s
            .enqueue_outbox(
                other,
                "b@example.com",
                &["q@y.io".to_owned()],
                "Other",
                b"raw-c",
                None,
                150,
            )
            .unwrap();

        // The scheduler drains a specific account, oldest first, with everything it needs to send.
        let pending = s.pending_outbox(acc).unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].id, a, "oldest first");
        assert_eq!(pending[0].recipients, vec!["x@y.io", "z@y.io"]);
        assert_eq!(pending[0].mail_from, "a@example.com");
        assert_eq!(pending[0].raw, b"raw-a");
        assert_eq!(
            pending[0].server_draft,
            Some(("Drafts".to_owned(), "<d1@geleit.local>".to_owned())),
            "the provider draft copy to clean up on delivery is carried"
        );
        assert_eq!(pending[1].id, b);
        assert_eq!(
            pending[1].server_draft, None,
            "an ordinary send carries none"
        );

        // A delivered message leaves the outbox; a rejected one is marked failed — no longer retried,
        // but not lost: it still counts, so the UI can surface it.
        s.delete_outbox(a).unwrap();
        s.mark_outbox_failed(b, "550 no such recipient").unwrap();
        assert!(
            s.pending_outbox(acc).unwrap().is_empty(),
            "a is gone, b is failed → nothing to retry"
        );
        assert_eq!(
            s.outbox_counts().unwrap(),
            (1, 1),
            "one queued (other account) + one failed"
        );

        // The outbox view lists everything (queued + failed) across accounts, **newest first**. By now a
        // (created_at 100) is discarded; b (200, this account, failed) and the other account's c (150)
        // remain — so newest-first is [b, c].
        let list = s.list_outbox().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, b, "newest (created_at 200) leads");
        assert_eq!(list[1].id, other_c, "then the older one (150)");
        let failed = list.iter().find(|o| o.id == b).unwrap();
        assert!(failed.failed);
        assert_eq!(failed.last_error.as_deref(), Some("550 no such recipient"));
        assert_eq!(failed.subject, "Later");
        assert!(
            list.iter().any(|o| !o.failed),
            "the other account's is still queued"
        );

        // Editing a rejected send reads its raw bytes straight back (to reopen it in the composer).
        assert_eq!(s.outbox_raw(b).unwrap().as_deref(), Some(&b"raw-b"[..]));
        assert_eq!(
            s.outbox_raw(999).unwrap(),
            None,
            "a row that's gone has no raw"
        );

        // A retry (or an immediate flush) needs to know which account a row belongs to.
        assert_eq!(s.outbox_account(b).unwrap(), Some(acc));
        assert_eq!(
            s.outbox_account(999).unwrap(),
            None,
            "a row that's gone has no account"
        );

        // Retry re-queues the failed one: it stops being failed and re-enters the drain.
        s.retry_outbox(b).unwrap();
        assert_eq!(s.outbox_counts().unwrap(), (2, 0), "failed → queued again");
        assert!(s.pending_outbox(acc).unwrap().iter().any(|m| m.id == b));

        // Removing the account takes its outbox with it (FK cascade).
        s.delete_account(other).unwrap();
        assert_eq!(
            s.outbox_counts().unwrap(),
            (1, 0),
            "only b remains, re-queued"
        );
    }

    #[test]
    fn a_change_made_while_the_writeback_is_in_flight_is_not_wrongly_settled() {
        // The race: the flush reads a message's flags to push them, the user changes them again, then
        // the flush tries to clear the marker. Clearing on the OLD value would settle a debt the server
        // doesn't reflect — and the pull would revert the user's newer change. The conditional clear
        // makes it a no-op, so the newer value stays queued.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();
        let id = s
            .upsert_message(
                acc,
                fld,
                &NewMessage {
                    uid: Some(1),
                    ..Default::default()
                },
            )
            .unwrap();

        s.set_seen(id, true).unwrap(); // read it — the flush would read (seen=true)
                                       // …the user marks it unread again before the flush clears the marker.
        s.set_seen(id, false).unwrap();
        // The flush, holding the stale (seen=true), tries to settle — and must NOT.
        assert!(
            !s.clear_flags_dirty(id, true, false).unwrap(),
            "the message moved on; the stale confirmation must not clear it"
        );
        assert_eq!(
            s.pending_flag_writebacks(acc).unwrap().len(),
            1,
            "still queued, so the next flush pushes the newer (unread) value"
        );
        // The next flush, with the current value, does settle it.
        assert!(s.clear_flags_dirty(id, false, false).unwrap());
        assert!(s.pending_flag_writebacks(acc).unwrap().is_empty());
    }

    #[test]
    fn a_local_flag_change_is_shielded_from_the_pull_until_its_writeback_confirms() {
        // The blocker SYNC-5 review caught: reading a message marks it read locally and writes `\Seen`
        // back on a worker. If a sync's flag pull ran before that landed, a naive server-wins pull
        // would flip the read straight back off. `set_seen` marks the flag dirty, and `flags_in_folder`
        // (the pull's local side) excludes dirty rows — so a just-made local change is never reverted.
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();
        let id = s
            .upsert_message(
                acc,
                fld,
                &NewMessage {
                    uid: Some(1),
                    seen: false,
                    ..Default::default()
                },
            )
            .unwrap();
        // Fresh from a sync: not dirty, so the pull would consider it.
        assert_eq!(s.flags_in_folder(fld).unwrap(), vec![(1, false, false)]);

        // The user reads it here. Now dirty → the pull leaves it alone (the server hasn't been told).
        s.set_seen(id, true).unwrap();
        assert!(
            s.flags_in_folder(fld).unwrap().is_empty(),
            "a message with an unconfirmed local change is shielded from the pull"
        );
        assert!(
            s.header_by_id(id).unwrap().unwrap().seen,
            "…and it IS read locally"
        );

        // The write-back confirms → no longer dirty → the pull reconciles it again, now finding
        // local and server in agreement.
        assert!(s.clear_flags_dirty(id, true, false).unwrap());
        assert_eq!(s.flags_in_folder(fld).unwrap(), vec![(1, true, false)]);

        // A star is shielded the same way.
        s.set_flagged(id, true).unwrap();
        assert!(s.flags_in_folder(fld).unwrap().is_empty());
    }

    #[test]
    fn imap_settings_roundtrip_update_and_default_none() {
        let s = Store::open_in_memory().unwrap();
        // plain account has no imap settings
        let plain = s.add_account("plain@example.com", None).unwrap();
        assert_eq!(s.imap_settings(plain).unwrap(), None);

        let settings = ImapSettings {
            host: "imap.example.com".to_owned(),
            port: 993,
            username: "me@example.com".to_owned(),
            allow_invalid_certs: false,
        };
        let acc = s
            .add_imap_account("me@example.com", Some("Me"), &settings)
            .unwrap();
        assert_eq!(s.imap_settings(acc).unwrap(), Some(settings));

        let updated = ImapSettings {
            host: "imap2.example.com".to_owned(),
            port: 143,
            username: "me2".to_owned(),
            allow_invalid_certs: true,
        };
        s.update_imap_settings(acc, &updated).unwrap();
        assert_eq!(s.imap_settings(acc).unwrap(), Some(updated));
    }

    #[test]
    fn smtp_settings_roundtrip_update_and_default_none() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("me@example.com", None).unwrap();
        // unconfigured → None
        assert_eq!(s.smtp_settings(acc).unwrap(), None);

        let starttls = SmtpConfig {
            host: "smtp.example.com".to_owned(),
            port: 587,
            security: SmtpSecurityKind::StartTls,
        };
        s.update_smtp_settings(acc, &starttls).unwrap();
        assert_eq!(s.smtp_settings(acc).unwrap(), Some(starttls));

        // update + the other security kind round-trips
        let implicit = SmtpConfig {
            host: "smtp2.example.com".to_owned(),
            port: 465,
            security: SmtpSecurityKind::Implicit,
        };
        s.update_smtp_settings(acc, &implicit).unwrap();
        assert_eq!(s.smtp_settings(acc).unwrap(), Some(implicit));
    }

    #[test]
    fn folder_priming_is_a_recorded_fact_defaulting_to_not_primed() {
        // "Primed" = this folder has completed a sync at least once. Until then, everything in it
        // looks new and must NOT be announced (a new account would notify about its whole inbox).
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@x.com", None).unwrap();
        let inbox = s.upsert_folder(acc, "INBOX").unwrap();
        let other = s.upsert_folder(acc, "Work").unwrap();

        // A folder we've never synced starts unprimed — the default matters, it's the safe one.
        assert!(!s.folder_primed(inbox).unwrap());

        s.set_folder_primed(inbox, true).unwrap();
        assert!(s.folder_primed(inbox).unwrap());
        assert!(!s.folder_primed(other).unwrap(), "priming is per folder");

        // A UIDVALIDITY reset un-primes it: every message looks new again, so we must go quiet again.
        s.set_folder_primed(inbox, false).unwrap();
        assert!(!s.folder_primed(inbox).unwrap());

        // Re-upserting the folder (every sync does) must not silently reset the flag.
        s.set_folder_primed(inbox, true).unwrap();
        assert_eq!(s.upsert_folder(acc, "INBOX").unwrap(), inbox);
        assert!(
            s.folder_primed(inbox).unwrap(),
            "upsert_folder must not un-prime"
        );
    }

    #[test]
    fn a_second_connection_can_write_while_the_first_reads() {
        // The app really does run several connections to one file (IPC + the engine's workers, which
        // open their own). Before WAL + busy_timeout this failed instantly with SQLITE_BUSY.
        let path =
            std::env::temp_dir().join(format!("geleit-concurrent-{}.db", std::process::id()));
        let path = path.to_str().unwrap();
        let _ = std::fs::remove_file(path);
        {
            let a = Store::open(path).unwrap();
            let acc = a.add_account("a@x.com", None).unwrap();
            let inbox = a.upsert_folder(acc, "INBOX").unwrap();

            // Both pragmas are on. WAL is a property of the *file*, so a second connection inherits
            // it; busy_timeout is per-connection, so every connection must set it — assert on both.
            // (Under WAL a reader never blocks a writer, so the scenario below can't exercise the
            // timeout; without this assert, dropping `busy_timeout` would go unnoticed. The timeout
            // is what saves writer-vs-writer, which two syncing accounts will do routinely.)
            let mode: String = a
                .conn
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(mode.to_lowercase(), "wal", "journal mode");
            let timeout: i64 = a
                .conn
                .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
                .unwrap();
            assert_eq!(timeout, 5000, "busy_timeout (connection A)");

            // Seed a row so the cursor below has something to stop on: an exhausted statement
            // finalizes and drops its read lock, which would make this test prove nothing.
            a.upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(1),
                    ..Default::default()
                },
            )
            .unwrap();

            // Hold a read transaction OPEN on connection A — a live statement parked mid-iteration,
            // so the shared lock is genuinely held while we write from elsewhere.
            let mut stmt = a.conn.prepare("SELECT id FROM message").unwrap();
            let mut rows = stmt.query([]).unwrap();
            assert!(
                rows.next().unwrap().is_some(),
                "cursor must be parked on a row"
            );

            // …while connection B writes. Under the old rollback journal this was SQLITE_BUSY.
            let b = Store::open(path).unwrap();
            let mode_b: String = b
                .conn
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(mode_b.to_lowercase(), "wal", "the file's mode, seen from B");
            let timeout_b: i64 = b
                .conn
                .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
                .unwrap();
            assert_eq!(timeout_b, 5000, "busy_timeout (connection B)");
            b.upsert_message(
                acc,
                inbox,
                &NewMessage {
                    uid: Some(2),
                    subject: Some("while you were reading".into()),
                    ..Default::default()
                },
            )
            .expect("a concurrent write must not fail with SQLITE_BUSY");
            assert_eq!(b.messages_in_folder(inbox, 10).unwrap().len(), 2);
        }
        // WAL leaves `-wal` / `-shm` sidecars next to the database — clean them up too.
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{path}{suffix}"));
        }
    }

    #[test]
    fn encryption_roundtrips_and_rejects_wrong_key_and_plaintext() {
        let path =
            std::env::temp_dir().join(format!("geleit-encryption-{}.db", std::process::id()));
        let path = path.to_str().unwrap();
        let _ = std::fs::remove_file(path);
        let key = [7u8; 32];
        let wrong = [9u8; 32];

        {
            let s = Store::open_encrypted(path, &key).unwrap();
            s.add_account("a@example.com", None).unwrap();
        }
        // same key → data is there
        {
            let s = Store::open_encrypted(path, &key).unwrap();
            assert_eq!(s.list_accounts().unwrap().len(), 1);
        }
        // wrong key → cannot open (proves it's actually encrypted)
        assert!(
            Store::open_encrypted(path, &wrong).is_err(),
            "wrong key must fail"
        );
        // opening it unencrypted → also fails (the file is ciphertext, not a plaintext DB)
        assert!(
            Store::open(path).and_then(|s| s.list_accounts()).is_err(),
            "plaintext open of an encrypted DB must fail"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn attachments_store_roundtrip_replace_and_cascade() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();
        let mid = s
            .upsert_message(
                acc,
                fld,
                &NewMessage {
                    uid: Some(1),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(s.attachments_for(mid).unwrap().is_empty());

        let atts = vec![
            Attachment {
                filename: Some("note.txt".to_owned()),
                content_type: "text/plain".to_owned(),
                size: 21,
            },
            Attachment {
                filename: None,
                content_type: "application/octet-stream".to_owned(),
                size: 100,
            },
        ];
        s.store_attachments(mid, &atts).unwrap();
        assert_eq!(s.attachments_for(mid).unwrap(), atts);

        // replace (re-sync) → no duplicates
        s.store_attachments(
            mid,
            &[Attachment {
                filename: Some("only.pdf".to_owned()),
                content_type: "application/pdf".to_owned(),
                size: 5,
            }],
        )
        .unwrap();
        let got = s.attachments_for(mid).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].filename.as_deref(), Some("only.pdf"));

        // cascade on message delete (the hot sync path)
        s.delete_messages_by_uid(fld, &[1]).unwrap();
        assert!(
            s.attachments_for(mid).unwrap().is_empty(),
            "cascade on message delete"
        );

        // cascade on account delete
        s.delete_account(acc).unwrap();
        assert!(s.attachments_for(mid).unwrap().is_empty());
    }

    #[test]
    fn message_id_and_in_reply_to_roundtrip() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();
        s.upsert_message(
            acc,
            fld,
            &NewMessage {
                uid: Some(1),
                message_id: Some("<b@x>".to_owned()),
                in_reply_to: Some("<a@x>".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        let h = &s.messages_in_folder(fld, 1).unwrap()[0];
        assert_eq!(h.message_id.as_deref(), Some("<b@x>"));
        assert_eq!(h.in_reply_to.as_deref(), Some("<a@x>"));
    }

    #[test]
    fn offline_read_returns_synced_mail() {
        // OFF-1: reading synced mail is a pure local-store operation — no network is involved, so
        // it works offline. (The whole `Store` read API is network-free by construction.)
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();
        let mid = s
            .upsert_message(
                acc,
                fld,
                &NewMessage {
                    uid: Some(1),
                    subject: Some("Offline subject".to_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        s.store_body(mid, Some("offline body"), None, Some("offline body"), false)
            .unwrap();

        let msgs = s.messages_in_folder(fld, 10).unwrap();
        assert_eq!(msgs[0].subject.as_deref(), Some("Offline subject"));
        assert_eq!(
            s.body_for(mid).unwrap().unwrap().plain.as_deref(),
            Some("offline body")
        );
    }

    #[test]
    fn delete_account_cascades() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();
        s.upsert_message(
            acc,
            fld,
            &NewMessage {
                uid: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        s.delete_account(acc).unwrap();
        assert!(s.list_accounts().unwrap().is_empty());
        assert!(s.folders_for_account(acc).unwrap().is_empty());
        assert!(s.messages_in_folder(fld, 10).unwrap().is_empty());
    }

    #[test]
    fn message_id_by_uid_absent_is_none() {
        let s = Store::open_in_memory().unwrap();
        let acc = s.add_account("a@example.com", None).unwrap();
        let fld = s.upsert_folder(acc, "INBOX").unwrap();
        assert_eq!(s.message_id_by_uid(acc, fld, 999).unwrap(), None);
    }

    #[test]
    fn invalid_email_error_carries_no_address() {
        // P2/§4: the message must not echo the address.
        assert_eq!(
            StoreError::InvalidEmail.to_string(),
            "invalid email address"
        );
    }
}
