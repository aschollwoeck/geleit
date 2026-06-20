//! Drives the engine's IMAP sync for **add-account** (`run_setup`) and **refresh** (`run_refresh`).
//! Both do network + blocking work and are meant to run on a **worker thread** (never the UI thread,
//! P1). Excluded from mutation testing (network/integration glue, like the engine's `imap.rs`); the
//! pure `build_settings` is unit-tested.
//!
//! Connection settings are persisted per-account in the store; the password lives in the OS
//! keychain via the shared `SecretStore` (`OsSecretStore` in the app — S2.1), so it persists
//! across restarts.

use geleit_engine::imap::{self, ImapConfig};
use geleit_platform::secret::SecretStore;
use geleit_store::{ImapSettings, Store, StoreError};

/// Validate raw Add-account form fields into `(email, ImapSettings)`. Pure — unit-tested. (Email
/// format is checked by the store on insert; here we reject empty host/username and bad ports.)
pub fn build_settings(
    email: &str,
    host: &str,
    port: &str,
    username: &str,
    allow_invalid_certs: bool,
) -> Result<(String, ImapSettings), String> {
    let email = email.trim();
    let host = host.trim();
    let username = username.trim();
    if email.is_empty() {
        return Err("Enter your email address.".to_owned());
    }
    if host.is_empty() {
        return Err("Enter your mail server (IMAP host).".to_owned());
    }
    if username.is_empty() {
        return Err("Enter your username.".to_owned());
    }
    let port: u16 = match port.trim() {
        "" => 993,
        p => p
            .parse()
            .ok()
            .filter(|&n| n != 0)
            .ok_or_else(|| "Enter a valid port (1–65535).".to_owned())?,
    };
    Ok((
        email.to_owned(),
        ImapSettings {
            host: host.to_owned(),
            port,
            username: username.to_owned(),
            allow_invalid_certs,
        },
    ))
}

fn to_config(s: &ImapSettings) -> ImapConfig {
    ImapConfig {
        host: s.host.clone(),
        port: s.port,
        username: s.username.clone(),
        allow_invalid_certs: s.allow_invalid_certs,
    }
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Couldn't start the sync runtime.".to_owned())
}

/// Add (or reconnect) an account: persist its settings, store the password in the shared secrets,
/// and do the first sync of the inbox. Blocking + network: **run on a worker thread.** A *newly*
/// created account is rolled back if the first connection fails, so a bad attempt leaves no trace.
pub fn run_setup(
    db_path: &str,
    secrets: &dyn SecretStore,
    email: &str,
    display_name: Option<&str>,
    settings: ImapSettings,
    password: &str,
) -> Result<(), String> {
    let store = Store::open(db_path).map_err(|_| "Couldn't open the local mailbox.".to_owned())?;
    // Single-account for now (M1): if an account already exists this is a reconnect/reconfigure —
    // update it rather than risk creating a hidden second account when the email field is edited.
    let existing = store
        .list_accounts()
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .into_iter()
        .next();
    let (account_id, is_new) = match existing {
        Some(a) => {
            store
                .update_imap_settings(a.id, &settings)
                .map_err(|_| "Couldn't save the account.".to_owned())?;
            (a.id, false)
        }
        None => {
            let id = store
                .add_imap_account(email, display_name, &settings)
                .map_err(|e| match e {
                    StoreError::InvalidEmail => "Enter a valid email address.".to_owned(),
                    _ => "Couldn't save the account.".to_owned(),
                })?;
            (id, true)
        }
    };

    if imap::store_password(secrets, &settings.username, password.as_bytes()).is_err() {
        if is_new {
            let _ = store.delete_account(account_id); // don't leave a half-created account
        }
        return Err("Couldn't store the password.".to_owned());
    }

    let config = to_config(&settings);
    let synced = runtime()?.block_on(async {
        imap::sync_folders(&config, secrets, &store, account_id).await?;
        imap::sync_folder_incremental(&config, secrets, &store, account_id, "INBOX", 200).await?;
        Ok::<(), imap::ImapError>(())
    });
    if synced.is_err() {
        if is_new {
            let _ = store.delete_account(account_id); // roll back a half-created account
        }
        // engine error discarded (that discard is the P2 safeguard); calm, actionable message (§10)
        return Err("Couldn't connect — check your details and try again.".to_owned());
    }
    Ok(())
}

/// Sync the first account's `folder` (+ folder list), reading settings from the store and the
/// password from the shared secrets. Blocking + network: **run on a worker thread.**
pub fn run_refresh(db_path: &str, secrets: &dyn SecretStore, folder: &str) -> Result<(), String> {
    let store = Store::open(db_path).map_err(|_| "Couldn't open the local mailbox.".to_owned())?;
    let account = store
        .list_accounts()
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .into_iter()
        .next()
        .ok_or_else(|| "No account configured yet.".to_owned())?;
    let settings = store
        .imap_settings(account.id)
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .ok_or_else(|| "This account isn't set up for syncing.".to_owned())?;

    let config = to_config(&settings);
    runtime()?
        .block_on(async {
            imap::sync_folders(&config, secrets, &store, account.id).await?;
            imap::sync_folder_incremental(&config, secrets, &store, account.id, folder, 200)
                .await?;
            Ok::<(), imap::ImapError>(())
        })
        .map_err(|_| "Couldn't refresh — check your connection and try again.".to_owned())
}

/// Progressively backfill the rest of `folder` (older messages) in the background, calling
/// `on_batch` with the running count after each batch. Reads settings from the store; blocking +
/// network → **run on a worker thread.**
pub fn run_backfill(
    db_path: &str,
    secrets: &dyn SecretStore,
    folder: &str,
    batch_size: u32,
    on_batch: &mut dyn FnMut(usize),
) -> Result<usize, String> {
    let store = Store::open(db_path).map_err(|_| "Couldn't open the local mailbox.".to_owned())?;
    let account = store
        .list_accounts()
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .into_iter()
        .next()
        .ok_or_else(|| "No account configured yet.".to_owned())?;
    let settings = store
        .imap_settings(account.id)
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .ok_or_else(|| "This account isn't set up for syncing.".to_owned())?;

    let config = to_config(&settings);
    runtime()?
        .block_on(imap::backfill_folder(
            &config, secrets, &store, account.id, folder, batch_size, on_batch,
        ))
        .map_err(|_| "Couldn't finish catching up — will resume next refresh.".to_owned())
}

/// Remove the (single) account from this device: delete its keychain password, then its local mail
/// (folders/messages/bodies cascade). Idempotent if there's no account. Touches the keychain
/// (D-Bus), so **run on a worker thread.**
///
/// Returns `Ok(true)` on a fully clean wipe, `Ok(false)` if the local mail was removed but the
/// keychain password could **not** be cleared (so the caller can warn — SEC-3), `Err` if the mail
/// wipe itself failed.
pub fn run_remove_account(db_path: &str, secrets: &dyn SecretStore) -> Result<bool, String> {
    let store = Store::open(db_path).map_err(|_| "Couldn't open the local mailbox.".to_owned())?;
    let Some(account) = store
        .list_accounts()
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .into_iter()
        .next()
    else {
        return Ok(true); // nothing to remove
    };
    // Forget the password (we still wipe the local mail even if this fails, but report it).
    let password_cleared = match store.imap_settings(account.id) {
        Ok(Some(settings)) => imap::delete_password(secrets, &settings.username).is_ok(),
        _ => true, // no stored password to clear
    };
    store
        .delete_account(account.id)
        .map_err(|_| "Couldn't remove the account.".to_owned())?;
    Ok(password_cleared)
}

#[cfg(test)]
mod tests {
    use super::build_settings;

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
        use super::run_remove_account;
        use geleit_engine::imap::{self, store_password};
        use geleit_platform::secret::InMemorySecretStore;
        use geleit_store::{ImapSettings, NewMessage, Store};

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
        {
            let store = Store::open(path).unwrap();
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
        }
        store_password(&secrets, "user@x.com", b"pw").unwrap();
        assert!(imap::has_password(&secrets, "user@x.com").unwrap());

        assert!(
            run_remove_account(path, &secrets).expect("remove"),
            "fully clean wipe"
        );

        let store = Store::open(path).unwrap();
        assert!(store.list_accounts().unwrap().is_empty(), "account gone");
        assert!(
            !imap::has_password(&secrets, "user@x.com").unwrap(),
            "password gone"
        );
        // removing again is a no-op (idempotent), still reported clean
        assert!(run_remove_account(path, &secrets).expect("remove again"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn passes_insecure_flag_through() {
        assert!(
            build_settings("me@x.com", "h", "993", "u", true)
                .unwrap()
                .1
                .allow_invalid_certs
        );
    }

    /// End-to-end against a local Dovecot: `run_setup` creates the account + syncs INBOX, then
    /// `run_refresh` reads the stored settings + session password and re-syncs.
    #[cfg(feature = "dangerous-tls")]
    #[test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    fn live_setup_then_refresh() {
        use super::{run_refresh, run_setup};
        use geleit_platform::secret::InMemorySecretStore;
        use geleit_store::{ImapSettings, Store};

        let path = std::env::temp_dir().join("geleit-setup-test.db");
        let path = path.to_str().unwrap();
        let _ = std::fs::remove_file(path);

        let secrets = InMemorySecretStore::new();
        let settings = ImapSettings {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        run_setup(
            path,
            &secrets,
            "geleittest@localhost",
            Some("geleittest"),
            settings,
            "testpass123",
        )
        .expect("setup");

        let store = Store::open(path).unwrap();
        let acc = store.list_accounts().unwrap()[0].id;
        let inbox = store
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .find(|f| f.name == "INBOX")
            .expect("INBOX synced")
            .id;
        assert!(!store.messages_in_folder(inbox, 10).unwrap().is_empty());
        drop(store);

        // refresh reads settings from the store + password from the shared secrets
        run_refresh(path, &secrets, "INBOX").expect("refresh");
        let _ = std::fs::remove_file(path);
    }
}
