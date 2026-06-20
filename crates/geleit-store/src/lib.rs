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
    fn invalid_email_error_carries_no_address() {
        // P2/§4: the message must not echo the address.
        assert_eq!(
            StoreError::InvalidEmail.to_string(),
            "invalid email address"
        );
    }
}
