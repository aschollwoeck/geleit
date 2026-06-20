//! Manual refresh: drives the engine's IMAP sync. `run_refresh` does network + blocking work and
//! is meant to run on a **worker thread** (never the UI thread, P1). Excluded from mutation testing
//! (network/integration glue, like the engine's `imap.rs`); `build_imap_config` is unit-tested.
//!
//! Connection settings come from the environment for now — the dev bridge until account-setup UI
//! (M7) and a real OS keychain land. The in-memory secret store here is dev-only.

use geleit_engine::imap::{self, ImapConfig};
use geleit_platform::secret::InMemorySecretStore;
use geleit_store::Store;

/// Build and validate an [`ImapConfig`] from raw parts. Pure — unit-tested.
pub fn build_imap_config(
    host: &str,
    port: &str,
    username: &str,
    allow_invalid_certs: bool,
) -> Result<ImapConfig, String> {
    let host = host.trim();
    let username = username.trim();
    if host.is_empty() {
        return Err("No mail server set (GELEIT_IMAP_HOST).".to_owned());
    }
    if username.is_empty() {
        return Err("No username set (GELEIT_IMAP_USER).".to_owned());
    }
    let port: u16 = match port.trim() {
        "" => 993,
        p => p
            .parse()
            .ok()
            .filter(|&n| n != 0)
            .ok_or_else(|| "Invalid port (GELEIT_IMAP_PORT).".to_owned())?,
    };
    Ok(ImapConfig {
        host: host.to_owned(),
        port,
        username: username.to_owned(),
        allow_invalid_certs,
    })
}

/// Read connection settings + password from the environment (dev bridge — see module docs).
pub fn config_from_env() -> Result<(ImapConfig, String), String> {
    let host = std::env::var("GELEIT_IMAP_HOST").unwrap_or_default();
    let port = std::env::var("GELEIT_IMAP_PORT").unwrap_or_default();
    let username = std::env::var("GELEIT_IMAP_USER").unwrap_or_default();
    let password = std::env::var("GELEIT_IMAP_PASSWORD").unwrap_or_default();
    let insecure = std::env::var("GELEIT_IMAP_INSECURE").is_ok();
    let config = build_imap_config(&host, &port, &username, insecure)?;
    Ok((config, password))
}

/// Sync the first account (folder list + `folder`'s envelopes and bodies) into the store at
/// `db_path`. Blocking + network: **run on a worker thread.** Returns a calm, PII-free message on
/// failure.
pub fn run_refresh(
    db_path: &str,
    config: ImapConfig,
    password: &str,
    folder: &str,
) -> Result<(), String> {
    let store = Store::open(db_path).map_err(|_| "Couldn't open the local mailbox.".to_owned())?;
    let account = store
        .list_accounts()
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .into_iter()
        .next()
        .ok_or_else(|| "No account configured yet.".to_owned())?;

    let secrets = InMemorySecretStore::new();
    imap::store_password(&secrets, &config.username, password.as_bytes())
        .map_err(|_| "Couldn't prepare credentials.".to_owned())?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Couldn't start the sync runtime.".to_owned())?;

    runtime
        .block_on(async {
            imap::sync_folders(&config, &secrets, &store, account.id).await?;
            imap::sync_envelopes(&config, &secrets, &store, account.id, folder, 200).await?;
            imap::sync_bodies(&config, &secrets, &store, account.id, folder, 200).await?;
            Ok::<(), imap::ImapError>(())
        })
        // The engine error is discarded entirely (that discard — not Display — is the P2 safeguard);
        // show a calm, actionable message instead (design.md §10).
        .map_err(|_| "Couldn't refresh — check your connection and try again.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::build_imap_config;

    #[test]
    fn valid_config() {
        let c = build_imap_config(" mail.example.com ", "993", " me ", false).unwrap();
        assert_eq!(c.host, "mail.example.com");
        assert_eq!(c.port, 993);
        assert_eq!(c.username, "me");
        assert!(!c.allow_invalid_certs);
    }

    #[test]
    fn empty_port_defaults_to_993() {
        assert_eq!(build_imap_config("h", "", "u", false).unwrap().port, 993);
    }

    #[test]
    fn rejects_empty_host_and_user() {
        assert!(build_imap_config("", "993", "u", false).is_err());
        assert!(build_imap_config("h", "993", "  ", false).is_err());
    }

    #[test]
    fn rejects_bad_port() {
        assert!(build_imap_config("h", "0", "u", false).is_err());
        assert!(build_imap_config("h", "70000", "u", false).is_err());
        assert!(build_imap_config("h", "abc", "u", false).is_err());
    }

    #[test]
    fn passes_insecure_flag_through() {
        assert!(
            build_imap_config("h", "993", "u", true)
                .unwrap()
                .allow_invalid_certs
        );
    }

    /// End-to-end refresh against a local Dovecot: an account in the store + `run_refresh` →
    /// INBOX gets messages. Exercises the real tokio runtime + engine sync the UI button drives.
    #[cfg(feature = "dangerous-tls")]
    #[test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    fn live_run_refresh_syncs_inbox() {
        use super::run_refresh;
        use geleit_store::Store;

        let path = std::env::temp_dir().join("geleit-refresh-test.db");
        let path = path.to_str().unwrap();
        let _ = std::fs::remove_file(path);

        let store = Store::open(path).unwrap();
        store
            .add_account("geleittest@localhost", Some("geleittest"))
            .unwrap();
        drop(store);

        let config = build_imap_config("127.0.0.1", "993", "geleittest", true).unwrap();
        run_refresh(path, config, "testpass123", "INBOX").expect("refresh");

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
        let _ = std::fs::remove_file(path);
    }
}
