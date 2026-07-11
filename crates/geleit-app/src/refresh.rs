//! Drives the engine's IMAP sync for **add-account** (`run_setup`) and **refresh** (`run_refresh`).
//! Both do network + blocking work and are meant to run on a **worker thread** (never the UI thread,
//! P1). Excluded from mutation testing (network/integration glue, like the engine's `imap.rs`); the
//! pure `build_settings` is unit-tested.
//!
//! Connection settings are persisted per-account in the store; the password lives in the OS
//! keychain via the shared `SecretStore` (`OsSecretStore` in the app — S2.1), so it persists
//! across restarts.

use geleit_engine::imap;
use geleit_platform::secret::SecretStore;

// The encrypted-store bootstrap moved to `geleit_engine::localstore` in S9.1 — it is UI-agnostic and
// the Tauri shell (M9) needs the identical logic. Re-exported so this module's callers are unchanged.
pub use geleit_engine::localstore::open_store;
// Message-action write-backs moved to `geleit_engine::sync_actions` in S9.3 (both UIs need them).
// Re-exported for this module's callers (main.rs) and used by the flows below.
use geleit_engine::sync_actions::{account_imap, runtime, to_config};
pub use geleit_engine::sync_actions::{
    build_settings, build_smtp_settings, run_backfill, run_delete_permanently, run_empty_folder,
    run_move, run_refresh, run_remove_account, run_send, run_set_flag, run_set_seen, run_setup,
};

/// Create / rename / delete a server folder (ORG-6), then re-sync that account's folder list so the
/// local rail reflects it. Blocking + network: **worker thread.** `op` runs the IMAP folder command.
fn folder_op(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    err: &str,
    op: impl std::future::Future<Output = Result<(), imap::ImapError>>,
) -> Result<(), String> {
    let store = open_store(db_path, secrets)?;
    let imap = store
        .imap_settings(account_id)
        .ok()
        .flatten()
        .ok_or_else(|| "This account isn't set up.".to_owned())?;
    let config = to_config(&imap);
    runtime()?
        .block_on(async {
            op.await?;
            imap::sync_folders(&config, secrets, &store, account_id).await // reconcile local list
        })
        .map_err(|_| err.to_owned())
}

/// Create a folder (ORG-6). Worker thread.
pub fn run_create_folder(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    name: &str,
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    folder_op(
        db_path,
        secrets,
        account_id,
        "Couldn't create the folder.",
        imap::create_folder(&config, secrets, name),
    )
}

/// Rename a folder (ORG-6). Worker thread.
pub fn run_rename_folder(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    from: &str,
    to: &str,
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    folder_op(
        db_path,
        secrets,
        account_id,
        "Couldn't rename the folder.",
        imap::rename_folder(&config, secrets, from, to),
    )
}

/// Delete a folder (ORG-6). Worker thread.
pub fn run_delete_folder(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    name: &str,
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    folder_op(
        db_path,
        secrets,
        account_id,
        "Couldn't delete the folder.",
        imap::delete_folder(&config, secrets, name),
    )
}

#[cfg(test)]
mod tests {
    use super::{build_settings, build_smtp_settings};
    use geleit_store::SmtpSecurityKind;

    #[test]
    fn smtp_defaults_and_security() {
        // STARTTLS, empty port → 587
        let s = build_smtp_settings(" smtp.example.com ", "", true).unwrap();
        assert_eq!(s.host, "smtp.example.com");
        assert_eq!(s.port, 587);
        assert_eq!(s.security, SmtpSecurityKind::StartTls);
        // implicit, empty port → 465
        let s = build_smtp_settings("smtp.example.com", "", false).unwrap();
        assert_eq!(s.port, 465);
        assert_eq!(s.security, SmtpSecurityKind::Implicit);
        // explicit port honoured
        assert_eq!(build_smtp_settings("h", "2525", false).unwrap().port, 2525);
    }

    #[test]
    fn smtp_rejects_empty_host_and_bad_port() {
        assert!(build_smtp_settings("  ", "587", true).is_err());
        assert!(build_smtp_settings("h", "0", false).is_err());
        assert!(build_smtp_settings("h", "abc", false).is_err());
    }

    #[test]
    fn valid_settings() {
        let (email, s) = build_settings(
            " me@example.com ",
            " mail.example.com ",
            "993",
            " me ",
            false,
        )
        .unwrap();
        assert_eq!(email, "me@example.com");
        assert_eq!(s.host, "mail.example.com");
        assert_eq!(s.port, 993);
        assert_eq!(s.username, "me");
        assert!(!s.allow_invalid_certs);
    }

    #[test]
    fn empty_port_defaults_to_993() {
        assert_eq!(
            build_settings("me@x.com", "h", "", "u", false)
                .unwrap()
                .1
                .port,
            993
        );
    }

    #[test]
    fn rejects_empty_fields() {
        assert!(build_settings("", "h", "993", "u", false).is_err());
        assert!(build_settings("me@x.com", "", "993", "u", false).is_err());
        assert!(build_settings("me@x.com", "h", "993", " ", false).is_err());
    }

    #[test]
    fn rejects_bad_port() {
        assert!(build_settings("me@x.com", "h", "0", "u", false).is_err());
        assert!(build_settings("me@x.com", "h", "70000", "u", false).is_err());
        assert!(build_settings("me@x.com", "h", "abc", "u", false).is_err());
    }

    #[test]
    fn run_remove_account_wipes_account_password_and_mail() {
        use super::{open_store, run_remove_account};
        use geleit_engine::imap::{self, store_password};
        use geleit_platform::secret::InMemorySecretStore;
        use geleit_store::{ImapSettings, NewMessage};

        let path = std::env::temp_dir().join("geleit-remove-test.db");
        let path = path.to_str().unwrap();
        let _ = std::fs::remove_file(path);

        let secrets = InMemorySecretStore::new();
        let settings = ImapSettings {
            host: "h".to_owned(),
            port: 993,
            username: "user@x.com".to_owned(),
            allow_invalid_certs: false,
        };
        let acc = {
            // encrypted store (open_store generates + stores the key in `secrets`)
            let store = open_store(path, &secrets).unwrap();
            let acc = store
                .add_imap_account("user@x.com", None, &settings)
                .unwrap();
            let fld = store.upsert_folder(acc, "INBOX").unwrap();
            let mid = store
                .upsert_message(
                    acc,
                    fld,
                    &NewMessage {
                        uid: Some(1),
                        ..Default::default()
                    },
                )
                .unwrap();
            store
                .store_body(mid, Some("body"), None, None, false)
                .unwrap();
            acc
        };
        store_password(&secrets, "user@x.com", b"pw").unwrap();
        assert!(imap::has_password(&secrets, "user@x.com").unwrap());

        assert!(
            run_remove_account(path, &secrets, acc).expect("remove"),
            "fully clean wipe"
        );

        let store = open_store(path, &secrets).unwrap();
        assert!(store.list_accounts().unwrap().is_empty(), "account gone");
        assert!(
            !imap::has_password(&secrets, "user@x.com").unwrap(),
            "password gone"
        );
        // removing again is a no-op (idempotent), still reported clean
        assert!(run_remove_account(path, &secrets, acc).expect("remove again"));
        let _ = std::fs::remove_file(path);
    }

    // `db_key` moved to `geleit_engine::localstore` in S9.1 (both UIs need it), and its tests moved
    // with it — where they also cover the guards this one didn't: a wrong-size key and a failing
    // keychain read must be *reported*, never overwritten.

    // The end-to-end setup+refresh live test moved to `geleit_engine::sync_actions`
    // (`live_setup_creates_and_syncs_an_account`): those functions now live there, and the old copy
    // here had been broken since encryption-at-rest — it opened the SQLCipher DB with `Store::open`
    // (unencrypted). Being `#[ignore]`, CI never caught it.
}
