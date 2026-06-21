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
];

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

/// A folder/mailbox row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub id: i64,
    pub account_id: i64,
    pub name: String,
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
    pub date: Option<i64>,
    pub seen: bool,
    pub has_attachments: bool,
    pub snippet: Option<String>,
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
    pub date: Option<i64>,
    pub seen: bool,
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
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let mut store = Self { conn };
        store.migrate()?;
        Ok(store)
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

    /// Delete an account and everything under it (folders/messages/bodies cascade).
    pub fn delete_account(&self, account_id: i64) -> Result<(), StoreError> {
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

    /// Folders for an account, ordered by name.
    pub fn folders_for_account(&self, account_id: i64) -> Result<Vec<Folder>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, name FROM folder WHERE account_id = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(Folder {
                id: r.get(0)?,
                account_id: r.get(1)?,
                name: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
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
        self.conn.execute(
            "INSERT INTO message \
             (account_id, folder_id, uid, message_id, in_reply_to, subject, from_name, from_addr, \
              date, seen, flagged, has_attachments, snippet) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?12) \
             ON CONFLICT(account_id, folder_id, uid) DO UPDATE SET \
               message_id = excluded.message_id, in_reply_to = excluded.in_reply_to, \
               subject = excluded.subject, from_name = excluded.from_name, \
               from_addr = excluded.from_addr, date = excluded.date, seen = excluded.seen",
            (
                account_id,
                folder_id,
                m.uid,
                &m.message_id,
                &m.in_reply_to,
                &m.subject,
                &m.from_name,
                &m.from_addr,
                m.date,
                m.seen,
                m.has_attachments,
                &m.snippet,
            ),
        )?;
        match m.uid {
            // On conflict the row is UPDATEd (not inserted), so look the id up by its unique key.
            Some(uid) => Ok(self.conn.query_row(
                "SELECT id FROM message WHERE account_id = ?1 AND folder_id = ?2 AND uid = ?3",
                (account_id, folder_id, uid),
                |r| r.get(0),
            )?),
            // A NULL uid never conflicts, so the row was just inserted.
            None => Ok(self.conn.last_insert_rowid()),
        }
    }

    /// Message headers for a folder, newest first (by date), up to `limit`.
    pub fn messages_in_folder(
        &self,
        folder_id: i64,
        limit: i64,
    ) -> Result<Vec<MessageHeader>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, uid, message_id, in_reply_to, subject, from_name, from_addr, date, seen, \
             has_attachments, snippet \
             FROM message WHERE folder_id = ?1 ORDER BY date DESC, id DESC LIMIT ?2",
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
                date: r.get(7)?,
                seen: r.get(8)?,
                has_attachments: r.get(9)?,
                snippet: r.get(10)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// A single message header by its store-row id (for reply/forward), or `None`.
    pub fn header_by_id(&self, id: i64) -> Result<Option<MessageHeader>, StoreError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, uid, message_id, in_reply_to, subject, from_name, from_addr, date, \
                 seen, has_attachments, snippet FROM message WHERE id = ?1",
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
                        date: r.get(7)?,
                        seen: r.get(8)?,
                        has_attachments: r.get(9)?,
                        snippet: r.get(10)?,
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
        Ok(())
    }

    /// Set a folder's IMAP UIDVALIDITY.
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

    /// Set a message's local read state. (Writing this back to the server is M6 / SYNC-5.)
    pub fn set_seen(&self, message_id: i64, seen: bool) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE message SET seen = ?2 WHERE id = ?1",
            (message_id, seen),
        )?;
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
                    ..Default::default()
                },
            )
            .unwrap();
        let h = s.header_by_id(id).unwrap().expect("found");
        assert_eq!(h.subject.as_deref(), Some("Hello"));
        assert_eq!(h.from_addr.as_deref(), Some("bob@x.com"));
        assert_eq!(h.message_id.as_deref(), Some("<m1@x>"));
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
    fn encryption_roundtrips_and_rejects_wrong_key_and_plaintext() {
        let path = std::env::temp_dir().join("geleit-encryption-test.db");
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
