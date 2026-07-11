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
use crate::message::{self, Draft};
use crate::smtp::{self, SmtpSecurity, SmtpSettings};
use geleit_platform::secret::SecretStore;
use geleit_store::{ImapSettings, SmtpSecurityKind};

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

/// Sync `account_id`'s `folder` (+ folder list), reading settings from the store and the password
/// from the shared secrets. Blocking + network: **run on a worker thread.**
pub fn run_refresh(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    folder: &str,
) -> Result<(), String> {
    let store = open_store(db_path, secrets)?;
    let settings = store
        .imap_settings(account_id)
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .ok_or_else(|| "This account isn't set up for syncing.".to_owned())?;

    let config = to_config(&settings);
    runtime()?
        .block_on(async {
            imap::sync_folders(&config, secrets, &store, account_id).await?;
            imap::sync_folder_incremental(&config, secrets, &store, account_id, folder, 200)
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
    account_id: i64,
    folder: &str,
    batch_size: u32,
    on_batch: &mut dyn FnMut(usize),
) -> Result<usize, String> {
    let store = open_store(db_path, secrets)?;
    let settings = store
        .imap_settings(account_id)
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .ok_or_else(|| "This account isn't set up for syncing.".to_owned())?;

    let config = to_config(&settings);
    runtime()?
        .block_on(imap::backfill_folder(
            &config, secrets, &store, account_id, folder, batch_size, on_batch,
        ))
        .map_err(|_| "Couldn't finish catching up — will resume next refresh.".to_owned())
}

/// Remove `account_id` from this device: delete its keychain password, then its local mail
/// (folders/messages/bodies cascade). Idempotent if the account is already gone. Touches the
/// keychain (D-Bus), so **run on a worker thread.**
///
/// Returns `Ok(true)` on a fully clean wipe, `Ok(false)` if the local mail was removed but the
/// keychain password could **not** be cleared (so the caller can warn — SEC-3), `Err` if the mail
/// wipe itself failed.
pub fn run_remove_account(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
) -> Result<bool, String> {
    let store = open_store(db_path, secrets)?;
    if store.account_by_id(account_id).ok().flatten().is_none() {
        return Ok(true); // nothing to remove
    }
    // Forget the password (we still wipe the local mail even if this fails, but report it).
    let password_cleared = match store.imap_settings(account_id) {
        Ok(Some(settings)) => imap::delete_password(secrets, &settings.username).is_ok(),
        _ => true, // no stored password to clear
    };
    store
        .delete_account(account_id)
        .map_err(|_| "Couldn't remove the account.".to_owned())?;
    Ok(password_cleared)
}

/// Split a comma/semicolon-separated address field into trimmed, non-empty addresses. Pure.
pub fn parse_addrs(field: &str) -> Vec<String> {
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
    account_id: i64,
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
        .account_by_id(account_id)
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
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

// Only a live (`dangerous-tls`) test lives here; without that feature the module is empty, so its
// imports are gated with it to avoid unused-import warnings in the default build.
#[cfg(all(test, feature = "dangerous-tls"))]
mod tests {
    use super::*;
    use geleit_platform::secret::InMemorySecretStore;
    use geleit_store::ImapSettings;

    /// The exact refresh + backfill path the Tauri `refresh` command drives (minus the event
    /// wrapper, which only forwards the `on_batch` count), against a local Dovecot. Proves the S9.3
    /// safety net actually pulls mail and streams progress.
    #[test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    fn live_refresh_then_backfill_streams_progress() {
        let path = std::env::temp_dir().join("geleit-s94-refresh-test.db");
        let path = path.to_str().unwrap();
        let _ = std::fs::remove_file(path);

        let secrets = InMemorySecretStore::new();
        let settings = ImapSettings {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        {
            let store = open_store(path, &secrets).expect("open");
            let acc = store
                .add_imap_account("geleittest@localhost", Some("geleittest"), &settings)
                .expect("add account");
            crate::imap::store_password(&secrets, "geleittest", b"testpass123").expect("password");

            run_refresh(path, &secrets, acc, "INBOX").expect("refresh");
            let inbox = store
                .folders_for_account(acc)
                .unwrap()
                .into_iter()
                .find(|f| f.name == "INBOX")
                .expect("INBOX synced")
                .id;
            assert!(
                !store.messages_in_folder(inbox, 10).unwrap().is_empty(),
                "recent sync pulled mail"
            );

            let mut batches = 0usize;
            let mut last = 0usize;
            run_backfill(path, &secrets, acc, "INBOX", 50, &mut |n| {
                batches += 1;
                last = n;
            })
            .expect("backfill");
            if last > 0 {
                assert!(batches >= 1, "backfill reported progress");
            }
        }
        let _ = std::fs::remove_file(path);
    }
}
