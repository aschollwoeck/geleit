//! `geleit-store` — the local SQLite store, the source of truth for the experience
//! (constitution P1). This crate owns the **account-scoped schema** and its migrations.
//!
//! Plain SQLite for now; **encryption at rest is M2** (SEC-1) and will wrap the connection open,
//! not the schema, so nothing here changes for it. UI-agnostic (ADR-0003); SQLite is bundled
//! (`rusqlite` `bundled` feature) so there is no system dependency.

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
];

/// An account row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: i64,
    pub email: String,
    pub display_name: Option<String>,
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

/// The local store (one SQLite connection).
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (or create) a store at `path`, enabling foreign keys and applying migrations.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> Result<Self, StoreError> {
        Self::init(Connection::open(path)?)
    }

    /// Open an in-memory store (tests / ephemeral use).
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
             (account_id, folder_id, uid, message_id, subject, from_name, from_addr, date, \
              seen, flagged, has_attachments, snippet) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11) \
             ON CONFLICT(account_id, folder_id, uid) DO UPDATE SET \
               message_id = excluded.message_id, subject = excluded.subject, \
               from_name = excluded.from_name, from_addr = excluded.from_addr, \
               date = excluded.date, seen = excluded.seen",
            (
                account_id,
                folder_id,
                m.uid,
                &m.message_id,
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
            "SELECT id, uid, subject, from_name, from_addr, date, seen, has_attachments, snippet \
             FROM message WHERE folder_id = ?1 ORDER BY date DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map((folder_id, limit), |r| {
            Ok(MessageHeader {
                id: r.get(0)?,
                uid: r.get(1)?,
                subject: r.get(2)?,
                from_name: r.get(3)?,
                from_addr: r.get(4)?,
                date: r.get(5)?,
                seen: r.get(6)?,
                has_attachments: r.get(7)?,
                snippet: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
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
