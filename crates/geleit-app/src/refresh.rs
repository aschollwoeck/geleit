//! Drives the engine's IMAP sync for **add-account** (`run_setup`) and **refresh** (`run_refresh`).
//! Both do network + blocking work and are meant to run on a **worker thread** (never the UI thread,
//! P1). Excluded from mutation testing (network/integration glue, like the engine's `imap.rs`); the
//! pure `build_settings` is unit-tested.
//!
//! Connection settings are persisted per-account in the store; the password lives in the OS
//! keychain via the shared `SecretStore` (`OsSecretStore` in the app — S2.1), so it persists
//! across restarts.

use geleit_engine::imap::{self, ImapConfig};
use geleit_engine::message::{self, Draft};
use geleit_engine::smtp::{self, SmtpSecurity, SmtpSettings};
use geleit_platform::secret::SecretStore;
use geleit_store::{ImapSettings, SmtpConfig, SmtpSecurityKind, Store, StoreError};

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

/// Validate raw SMTP form fields into an `SmtpConfig`. Pure — unit-tested. An empty port defaults to
/// the standard for the chosen security (465 implicit / 587 STARTTLS).
pub fn build_smtp_settings(host: &str, port: &str, starttls: bool) -> Result<SmtpConfig, String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("Enter your outgoing mail server (SMTP host).".to_owned());
    }
    let security = if starttls {
        SmtpSecurityKind::StartTls
    } else {
        SmtpSecurityKind::Implicit
    };
    let port: u16 = match port.trim() {
        "" if starttls => 587,
        "" => 465,
        p => p
            .parse()
            .ok()
            .filter(|&n| n != 0)
            .ok_or_else(|| "Enter a valid SMTP port (1–65535).".to_owned())?,
    };
    Ok(SmtpConfig {
        host: host.to_owned(),
        port,
        security,
    })
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

const DB_KEY_SERVICE: &str = "geleit-db";
const DB_KEY_ACCOUNT: &str = "key";

/// The database encryption key (SEC-1, ADR-0008): fetched from the keychain, or a fresh 32-byte
/// random key generated and stored there on first run. Never logged (P2).
///
/// Only generates a key when the keychain reports the entry is genuinely **absent** — a read error
/// or a present-but-wrong-size key is surfaced, never overwritten, so a transient keychain failure
/// can't discard the real key and brick the encrypted DB.
pub fn db_key(secrets: &dyn SecretStore) -> Result<Vec<u8>, String> {
    match secrets.get(DB_KEY_SERVICE, DB_KEY_ACCOUNT) {
        Ok(Some(key)) if key.len() == 32 => return Ok(key),
        Ok(Some(_)) => return Err("The stored encryption key looks corrupt.".to_owned()),
        Ok(None) => {} // first run → generate below
        Err(_) => return Err("Couldn't read the encryption key from the keychain.".to_owned()),
    }
    let mut key = vec![0u8; 32];
    getrandom::fill(&mut key).map_err(|_| "Couldn't generate an encryption key.".to_owned())?;
    secrets
        .set(DB_KEY_SERVICE, DB_KEY_ACCOUNT, &key)
        .map_err(|_| "Couldn't store the encryption key.".to_owned())?;
    Ok(key)
}

/// Open the **encrypted** local store, fetching (or creating) its key from the keychain.
pub fn open_store(db_path: &str, secrets: &dyn SecretStore) -> Result<Store, String> {
    let key = db_key(secrets)?;
    Store::open_encrypted(db_path, &key)
        .map_err(|_| "Couldn't open the encrypted mailbox.".to_owned())
}

/// Add (or reconnect) an account: persist its settings, store the password in the shared secrets,
/// and do the first sync of the inbox. Blocking + network: **run on a worker thread.** A *newly*
/// created account is rolled back if the first connection fails, so a bad attempt leaves no trace.
#[allow(clippy::too_many_arguments)]
pub fn run_setup(
    db_path: &str,
    secrets: &dyn SecretStore,
    email: &str,
    display_name: Option<&str>,
    settings: ImapSettings,
    smtp: SmtpConfig,
    signature: &str,
    password: &str,
) -> Result<(), String> {
    let store = open_store(db_path, secrets)?;
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
    // Persist SMTP settings + signature (sending, M4). A failure on a new account rolls back below.
    if store.update_smtp_settings(account_id, &smtp).is_err()
        || store.update_signature(account_id, signature).is_err()
    {
        if is_new {
            let _ = store.delete_account(account_id);
        }
        return Err("Couldn't save the account.".to_owned());
    }

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

/// Split a comma/semicolon-separated address field into trimmed, non-empty addresses. Pure.
fn parse_addrs(field: &str) -> Vec<String> {
    field
        .split([',', ';'])
        .map(|a| a.trim().to_owned())
        .filter(|a| !a.is_empty())
        .collect()
}

/// Send a composed message via the first account's SMTP server (M4). Reads SMTP settings from the
/// store and reuses the IMAP username/password from the keychain. Blocking + network: **run on a
/// worker thread.** Calm, PII-free errors.
#[allow(clippy::too_many_arguments)]
pub fn run_send(
    db_path: &str,
    secrets: &dyn SecretStore,
    to: &str,
    cc: &str,
    subject: &str,
    body: &str,
    in_reply_to: Option<String>,
    references: Vec<String>,
    attachments: Vec<message::Attachment>,
    markdown: bool,
    draft_id: Option<i64>,
) -> Result<(), String> {
    let store = open_store(db_path, secrets)?;
    let account = store
        .list_accounts()
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .into_iter()
        .next()
        .ok_or_else(|| "No account is set up yet.".to_owned())?;
    let imap = store
        .imap_settings(account.id)
        .ok()
        .flatten()
        .ok_or_else(|| "This account isn't set up.".to_owned())?;
    let smtp = store
        .smtp_settings(account.id)
        .ok()
        .flatten()
        .ok_or_else(|| "No outgoing (SMTP) server is configured for this account.".to_owned())?;

    let password = imap::password(secrets, &imap.username)
        .map_err(|_| "Couldn't read your saved password.".to_owned())?
        .ok_or_else(|| "Enter your password (Refresh to reconnect) before sending.".to_owned())?;
    let password =
        String::from_utf8(password).map_err(|_| "The saved password looks corrupt.".to_owned())?;

    let draft = Draft {
        from_name: account.display_name.clone(),
        from_addr: account.email.clone(),
        to: parse_addrs(to),
        cc: parse_addrs(cc),
        subject: subject.to_owned(),
        body_text: body.to_owned(),
        in_reply_to,
        references,
        attachments,
        html_body: markdown.then(|| message::render_markdown(body)),
    };
    let bytes = message::build(&draft)?;
    let envelope = smtp::envelope(&draft.from_addr, &message::recipients(&draft))?;
    let settings = SmtpSettings {
        host: smtp.host,
        port: smtp.port,
        username: imap.username.clone(),
        security: match smtp.security {
            SmtpSecurityKind::Implicit => SmtpSecurity::Implicit,
            SmtpSecurityKind::StartTls => SmtpSecurity::StartTls,
        },
        allow_invalid_certs: imap.allow_invalid_certs,
    };
    // A Sent folder to save a copy in (SEND-8), by name (SPECIAL-USE detection is a follow-up).
    let sent_folder = store.folders_for_account(account.id).ok().and_then(|fs| {
        fs.into_iter()
            .map(|f| f.name)
            .find(|n| n.eq_ignore_ascii_case("sent") || n.to_ascii_lowercase().contains("sent"))
    });
    let imap_config = to_config(&imap);
    runtime()?.block_on(async {
        smtp::send(&settings, &password, &envelope, &bytes).await?;
        // Best-effort: the message is already sent; failing to save a Sent copy must not report
        // failure (it would mislead the person into resending).
        if let Some(folder) = sent_folder {
            let _ = imap::append_message(&imap_config, secrets, &folder, &bytes).await;
        }
        Ok::<(), String>(())
    })?;
    // The message went out — drop the draft it came from (best-effort).
    if let Some(id) = draft_id {
        let _ = store.delete_draft(id);
    }
    Ok(())
}

/// Write a star (`\Flagged`) change back to the server (ORG-4). Blocking + network: **worker thread.**
pub fn run_set_flag(
    db_path: &str,
    secrets: &dyn SecretStore,
    folder: &str,
    uid: u32,
    flagged: bool,
) -> Result<(), String> {
    let store = open_store(db_path, secrets)?;
    let account = store
        .list_accounts()
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .into_iter()
        .next()
        .ok_or_else(|| "No account is set up yet.".to_owned())?;
    let imap = store
        .imap_settings(account.id)
        .ok()
        .flatten()
        .ok_or_else(|| "This account isn't set up.".to_owned())?;
    let config = to_config(&imap);
    runtime()?
        .block_on(imap::set_flag(&config, secrets, folder, uid, flagged))
        .map_err(|_| "Couldn't update the star on the server.".to_owned())
}

/// Sync the first account's `folder` (+ folder list), reading settings from the store and the
/// password from the shared secrets. Blocking + network: **run on a worker thread.**
pub fn run_refresh(db_path: &str, secrets: &dyn SecretStore, folder: &str) -> Result<(), String> {
    let store = open_store(db_path, secrets)?;
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
    let store = open_store(db_path, secrets)?;
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
    let store = open_store(db_path, secrets)?;
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
    fn parse_addrs_splits_trims_and_drops_empties() {
        use super::parse_addrs;
        assert_eq!(
            parse_addrs(" a@x.com , b@y.com ;c@z.com,"),
            vec!["a@x.com", "b@y.com", "c@z.com"]
        );
        assert!(parse_addrs("   ").is_empty());
        assert!(parse_addrs("").is_empty());
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
        {
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
        }
        store_password(&secrets, "user@x.com", b"pw").unwrap();
        assert!(imap::has_password(&secrets, "user@x.com").unwrap());

        assert!(
            run_remove_account(path, &secrets).expect("remove"),
            "fully clean wipe"
        );

        let store = open_store(path, &secrets).unwrap();
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
    fn db_key_is_32_bytes_and_stable() {
        use super::db_key;
        use geleit_platform::secret::InMemorySecretStore;

        let secrets = InMemorySecretStore::new();
        let k1 = db_key(&secrets).unwrap();
        assert_eq!(k1.len(), 32);
        let k2 = db_key(&secrets).unwrap();
        assert_eq!(k1, k2, "key persists, not regenerated each call");
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
        use geleit_store::{ImapSettings, SmtpConfig, SmtpSecurityKind, Store};

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
        let smtp = SmtpConfig {
            host: "127.0.0.1".to_owned(),
            port: 465,
            security: SmtpSecurityKind::Implicit,
        };
        run_setup(
            path,
            &secrets,
            "geleittest@localhost",
            Some("geleittest"),
            settings,
            smtp,
            "",
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
