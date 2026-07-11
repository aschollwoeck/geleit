//! Server write-backs for message actions — star, read-state, move, delete (M9 S9.3).
//!
//! Moved here from the Slint app's `refresh.rs` (S9.3): they are UI-agnostic, and the Tauri shell
//! needs the identical logic. Each is a thin, blocking wrapper over the real IMAP logic in
//! [`crate::imap`] — open the store, read the account's IMAP config, `block_on` the network call.
//! **Blocking + network: always call these on a worker thread**, never the UI thread (P1).
//!
//! The optimistic-then-write-back model (M5): the UI updates the local store immediately; these run
//! in the background; a failure self-heals on the next refresh. They never expunge on the optimistic
//! path, so a failed write-back can't lose mail.
use crate::imap::{self, ImapConfig};
use crate::localstore::open_store;
use geleit_platform::secret::SecretStore;
use geleit_store::ImapSettings;

/// Map stored IMAP settings to a connection config.
pub fn to_config(s: &ImapSettings) -> ImapConfig {
    ImapConfig {
        host: s.host.clone(),
        port: s.port,
        username: s.username.clone(),
        allow_invalid_certs: s.allow_invalid_certs,
    }
}

/// A single-threaded Tokio runtime for one blocking network call.
pub fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Couldn't start the sync runtime.".to_owned())
}

/// The IMAP connection config for an account, read from the encrypted store.
pub fn account_imap(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
) -> Result<ImapConfig, String> {
    let store = open_store(db_path, secrets)?;
    let imap = store
        .imap_settings(account_id)
        .ok()
        .flatten()
        .ok_or_else(|| "This account isn't set up.".to_owned())?;
    Ok(to_config(&imap))
}

/// Write a star (`\Flagged`) change back to the server (ORG-4). Blocking + network: **worker thread.**
pub fn run_set_flag(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    folder: &str,
    uid: u32,
    flagged: bool,
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    runtime()?
        .block_on(imap::set_flag(&config, secrets, folder, uid, flagged))
        .map_err(|_| "Couldn't update the star on the server.".to_owned())
}

/// Write a read-state (`\Seen`) change back to the server (SYNC-5). Blocking + network: **worker thread.**
pub fn run_set_seen(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    folder: &str,
    uid: u32,
    seen: bool,
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    runtime()?
        .block_on(imap::set_seen(&config, secrets, folder, uid, seen))
        .map_err(|_| "Couldn't update read state on the server.".to_owned())
}

/// Move a message by UID from `source` to `target` on the server (ORG-1/2/3 write-back). Blocking +
/// network: **worker thread.**
pub fn run_move(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    source: &str,
    uid: u32,
    target: &str,
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    runtime()?
        .block_on(imap::move_message(&config, secrets, source, uid, target))
        .map_err(|_| "Couldn't move the message on the server.".to_owned())
}

/// Permanently delete one message by UID (ORG-2). Blocking + network: **worker thread.**
pub fn run_delete_permanently(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    folder: &str,
    uid: u32,
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    runtime()?
        .block_on(imap::delete_permanently(&config, secrets, folder, uid))
        .map_err(|_| "Couldn't delete the message on the server.".to_owned())
}

/// Empty a folder on the server (ORG-2, empty-trash). Blocking + network: **worker thread.**
pub fn run_empty_folder(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    folder: &str,
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    runtime()?
        .block_on(imap::empty_folder(&config, secrets, folder))
        .map_err(|_| "Couldn't empty the folder on the server.".to_owned())
}
