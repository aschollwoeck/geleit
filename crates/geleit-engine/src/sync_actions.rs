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
use geleit_store::{ImapSettings, SmtpConfig, SmtpSecurityKind, Store, StoreError};
use lettre::address::Envelope;

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

/// Drain the account's durable flag-write-back queue (SYNC-5): push every message with an unconfirmed
/// local read/star change to the server, clearing the dirty marker on the ones that land.
///
/// This is what makes a flag change made **offline** eventually reach the server — the scheduler calls
/// it every sweep, and it survives restarts because the queue is just the `flags_dirty` rows in the
/// store. One session per folder; a folder we can't reach leaves its messages dirty for next time.
/// Returns how many messages were confirmed. Blocking + network: **worker thread.**
pub fn run_flush_flags(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
) -> Result<usize, String> {
    let store = open_store(db_path, secrets)?;
    let pending = store
        .pending_flag_writebacks(account_id)
        .map_err(|_| "Couldn't read pending changes.".to_owned())?;
    if pending.is_empty() {
        return Ok(0);
    }
    let config = account_imap(db_path, secrets, account_id)?;
    let rt = runtime()?;

    // Group by folder so each folder is one session; keep a (folder, uid) → message-id map to clear
    // the dirty marker for exactly the messages that were pushed.
    let mut by_folder: std::collections::HashMap<String, Vec<(u32, bool, bool)>> =
        std::collections::HashMap::new();
    // (folder, uid) → (message id, the flags we pushed) so the clear is conditional on those exact
    // flags — see `clear_flags_dirty`.
    let mut meta: std::collections::HashMap<(String, u32), (i64, bool, bool)> =
        std::collections::HashMap::new();
    for p in pending {
        let uid = p.uid as u32;
        by_folder
            .entry(p.folder.clone())
            .or_default()
            .push((uid, p.seen, p.flagged));
        meta.insert((p.folder, uid), (p.id, p.seen, p.flagged));
    }

    let mut confirmed = 0usize;
    for (folder, items) in by_folder {
        // A folder that won't push (offline, gone) leaves its messages dirty — the queue retries them.
        let Ok(pushed) = rt.block_on(imap::push_flags(&config, secrets, &folder, &items)) else {
            continue;
        };
        for uid in pushed {
            if let Some(&(id, seen, flagged)) = meta.get(&(folder.clone(), uid)) {
                // Clears only if the flags haven't moved since we read them — if the user changed them
                // again mid-flush, the row stays queued and the next flush pushes the newer value.
                if store.clear_flags_dirty(id, seen, flagged).unwrap_or(false) {
                    confirmed += 1;
                }
            }
        }
    }
    Ok(confirmed)
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

/// Fetch one attachment's bytes on demand (READ-8, save to disk). Fetches the whole raw message by
/// UID and extracts the `index`-th attachment. Returns its filename (if the message names it) and
/// bytes. Blocking + network: **worker thread.**
pub fn run_fetch_attachment(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    folder: &str,
    uid: u32,
    index: usize,
) -> Result<(Option<String>, Vec<u8>), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    let raw = runtime()?
        .block_on(imap::fetch_raw_message(&config, secrets, folder, uid))
        .map_err(|_| "Couldn't fetch the attachment from the server.".to_owned())?
        .ok_or_else(|| "That message is no longer on the server.".to_owned())?;
    let att = crate::mime::extract_attachment(&raw, index)
        .ok_or_else(|| "Couldn't find that attachment in the message.".to_owned())?;
    Ok((att.filename, att.data))
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

/// Create a folder on the server (ORG-6). The local folder row is added by the caller. Blocking +
/// network: **worker thread.**
pub fn run_create_folder(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    name: &str,
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    runtime()?
        .block_on(imap::create_folder(&config, secrets, name))
        .map_err(|_| "Couldn't create the folder on the server.".to_owned())
}

/// Rename a folder on the server (ORG-6). The caller renames the local row in place. Blocking +
/// network: **worker thread.**
pub fn run_rename_folder(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    from: &str,
    to: &str,
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    runtime()?
        .block_on(imap::rename_folder(&config, secrets, from, to))
        .map_err(|_| "Couldn't rename the folder on the server.".to_owned())
}

/// Delete a folder on the server (ORG-6). The caller removes the local row (cascading its messages).
/// Blocking + network: **worker thread.**
pub fn run_delete_folder(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    name: &str,
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    runtime()?
        .block_on(imap::delete_folder(&config, secrets, name))
        .map_err(|_| "Couldn't delete the folder on the server.".to_owned())
}

/// Sync `account_id`'s `folder` (+ folder list), reading settings from the store and the password
/// from the shared secrets. Blocking + network: **run on a worker thread.**
pub fn run_refresh(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    folder: &str,
) -> Result<imap::SyncOutcome, String> {
    let store = open_store(db_path, secrets)?;
    let settings = store
        .imap_settings(account_id)
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .ok_or_else(|| "This account isn't set up for syncing.".to_owned())?;

    let config = to_config(&settings);
    runtime()?
        .block_on(async {
            imap::sync_folders(&config, secrets, &store, account_id).await?;
            // The outcome carries WHICH messages arrived (and whether the folder was primed), so the
            // caller can tell the user about them (NOTIF-1). It used to be discarded.
            imap::sync_folder_incremental(&config, secrets, &store, account_id, folder, 200).await
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
/// What became of a send (SEND-10): it went out, or it couldn't be delivered now and is waiting in the
/// outbox to be retried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendStatus {
    Sent,
    Queued,
}

/// Everything an SMTP delivery needs for an account: the transport settings, the password, an IMAP
/// config + the Sent folder to file a copy in. Resolved once, shared by the compose path and the
/// outbox drain.
struct SendContext {
    settings: SmtpSettings,
    password: String,
    imap_config: ImapConfig,
    sent_folder: Option<String>,
}

fn send_context(
    store: &Store,
    secrets: &dyn SecretStore,
    account_id: i64,
) -> Result<SendContext, String> {
    let imap = store
        .imap_settings(account_id)
        .ok()
        .flatten()
        .ok_or_else(|| "This account isn't set up.".to_owned())?;
    let smtp = store
        .smtp_settings(account_id)
        .ok()
        .flatten()
        .ok_or_else(|| "No outgoing (SMTP) server is configured for this account.".to_owned())?;
    let password = imap::password(secrets, &imap.username)
        .map_err(|_| "Couldn't read your saved password.".to_owned())?
        .ok_or_else(|| "Enter your password (Refresh to reconnect) before sending.".to_owned())?;
    let password =
        String::from_utf8(password).map_err(|_| "The saved password looks corrupt.".to_owned())?;
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
    // A Sent folder to save a copy in (SEND-8), by the server's own `\Sent` flag (RFC 6154) with the
    // English name as fallback — so `Gesendet` etc. still resolve.
    let sent_folder = store.folders_for_account(account_id).ok().and_then(|fs| {
        let pairs: Vec<(String, Option<geleit_core::FolderRole>)> = fs
            .into_iter()
            .map(|f| {
                let role = f
                    .role
                    .as_deref()
                    .and_then(geleit_core::FolderRole::from_key);
                (f.name, role)
            })
            .collect();
        geleit_core::pick_folder(&pairs, geleit_core::FolderRole::Sent).map(str::to_owned)
    });
    Ok(SendContext {
        settings,
        password,
        imap_config: to_config(&imap),
        sent_folder,
    })
}

/// Deliver already-built bytes to an SMTP server and, on success, file a copy in Sent (best-effort:
/// the mail is gone, so a failed Sent-save must not report failure). The one delivery choke-point,
/// shared by the compose path and the outbox drain.
async fn deliver(
    ctx: &SendContext,
    secrets: &dyn SecretStore,
    envelope: &Envelope,
    bytes: &[u8],
) -> Result<(), smtp::SendError> {
    smtp::send(&ctx.settings, &ctx.password, envelope, bytes).await?;
    if let Some(folder) = &ctx.sent_folder {
        let _ = imap::append_message(&ctx.imap_config, secrets, folder, "(\\Seen)", bytes).await;
    }
    Ok(())
}

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
    outbox_edit_id: Option<i64>,
) -> Result<SendStatus, String> {
    let store = open_store(db_path, secrets)?;
    let account = store
        .account_by_id(account_id)
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?
        .ok_or_else(|| "No account is set up yet.".to_owned())?;
    let ctx = send_context(&store, secrets, account_id)?;

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
    let recipients = message::recipients(&draft);
    let envelope = smtp::envelope(&draft.from_addr, &recipients)?;
    // If this draft has a copy on the server (opt-in "sync drafts"), remove it once the mail is away.
    // By the draft's **stored** Message-ID — deriving one from the row id would expunge whatever copy a
    // long-dead draft with the same (reused) id left behind (store migration 15).
    let server_draft = draft_id.and_then(|id| {
        let row = store.draft_by_id(id).ok().flatten()?;
        Some((row.server_folder?, row.msgid))
    });

    // Try to deliver now. A `permanent` failure (the server rejected it) is surfaced — queuing a
    // rejected message would loop forever; the user must fix or drop it. Any other failure is the
    // ordinary offline case, so the message goes to the outbox and the scheduler retries it.
    let result = runtime()?.block_on(async {
        match deliver(&ctx, secrets, &envelope, &bytes).await {
            Ok(()) => {
                if let Some((folder, mid)) = &server_draft {
                    let _ = imap::expunge_draft(&ctx.imap_config, secrets, folder, mid).await;
                }
                Ok(SendStatus::Sent)
            }
            Err(e) if e.permanent => Err(e.message),
            Err(_) => Ok(SendStatus::Queued),
        }
    })?;

    if result == SendStatus::Queued {
        let now = now_secs();
        let sd = server_draft
            .as_ref()
            .map(|(folder, mid)| (folder.as_str(), mid.as_str()));
        store
            .enqueue_outbox(
                account.id,
                &draft.from_addr,
                &recipients,
                &draft.subject,
                &bytes,
                sd,
                now,
            )
            .map_err(|_| "Couldn't queue the message to send.".to_owned())?;
    }
    // The message is either sent or safely queued — either way drop the draft it came from
    // (best-effort; its content lives on in the outbox if queued).
    if let Some(id) = draft_id {
        let _ = store.delete_draft(id);
    }
    // Likewise drop the rejected outbox row this send was an edit of (SEND-10 edit). Doing it here,
    // in the same worker that just enqueued/sent the fresh copy, closes the window where the original
    // could linger and be retried into a duplicate — the resend replaces it, never doubles it.
    if let Some(id) = outbox_edit_id {
        let _ = store.delete_outbox(id);
    }
    Ok(result)
}

/// Drain an account's outbox (SEND-10): try to deliver each queued message; a delivered one leaves the
/// outbox, a rejected one is marked failed (so it stops retrying and can be surfaced), a
/// still-unreachable one stays for the next sweep. Returns how many went out. Blocking + network:
/// **worker thread.**
pub fn run_flush_outbox(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
) -> Result<usize, String> {
    let store = open_store(db_path, secrets)?;
    let pending = store
        .pending_outbox(account_id)
        .map_err(|_| "Couldn't read the outbox.".to_owned())?;
    if pending.is_empty() {
        return Ok(0);
    }
    // Resolve settings once; if the account can't even be set up (no SMTP configured), there's nothing
    // to retry against — leave the messages queued rather than marking them failed.
    let Ok(ctx) = send_context(&store, secrets, account_id) else {
        return Ok(0);
    };
    let rt = runtime()?;

    let mut sent = 0usize;
    for msg in pending {
        let Ok(envelope) = smtp::envelope(&msg.mail_from, &msg.recipients) else {
            // A stored envelope that no longer parses can never send — surface it rather than loop.
            let _ = store.mark_outbox_failed(msg.id, "The saved recipients are invalid.");
            continue;
        };
        match rt.block_on(deliver(&ctx, secrets, &envelope, &msg.raw)) {
            Ok(()) => {
                let _ = store.delete_outbox(msg.id);
                sent += 1;
            }
            // Rejected on retry (rare — it was queued for a connection problem): stop retrying it.
            Err(e) if e.permanent => {
                let _ = store.mark_outbox_failed(msg.id, &e.message);
            }
            // Still can't reach the server — leave it queued for the next sweep.
            Err(_) => {}
        }
    }
    Ok(sent)
}

/// Unix seconds now, for stamping an outbox row. A worker thread, so the real clock is fine (unlike
/// the workflow scripts).
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Save a draft's copy to the server's Drafts folder (SEND-5, opt-in)/// Save a draft's copy to the server's Drafts folder (SEND-5, opt-in): replaces whatever copy of this
/// draft is there (matched by its stable Message-ID) with fresh `\Draft` bytes, in one session.
/// Blocking + network: **worker.**
pub fn run_sync_draft(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    folder: &str,
    message_id: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    runtime()?
        .block_on(imap::sync_draft(
            &config, secrets, folder, message_id, bytes,
        ))
        .map_err(|_| "Couldn't save the draft to the server.".to_owned())
}

/// Remove a draft's copy from the server (SEND-5, opt-in) — on send, discard, or when the setting is
/// switched off. Idempotent. Blocking + network: **worker.**
pub fn run_expunge_server_draft(
    db_path: &str,
    secrets: &dyn SecretStore,
    account_id: i64,
    folder: &str,
    message_id: &str,
) -> Result<(), String> {
    let config = account_imap(db_path, secrets, account_id)?;
    runtime()?
        .block_on(imap::expunge_draft(&config, secrets, folder, message_id))
        .map_err(|_| "Couldn't remove the draft from the server.".to_owned())
}

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

/// Keyed by **email**: re-running with an existing email reconfigures that account; a new email adds
/// a new account (ACC-5, multi-account).
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
) -> Result<i64, String> {
    let store = open_store(db_path, secrets)?;
    let existing = store
        .account_by_email(email.trim())
        .map_err(|_| "Couldn't read the local mailbox.".to_owned())?;
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
    Ok(account_id)
}

#[cfg(test)]
mod pure_tests {
    use super::parse_addrs;

    #[test]
    fn parse_addrs_splits_trims_and_drops_empties() {
        assert_eq!(
            parse_addrs(" a@x.com , b@y.com ;c@z.com,"),
            vec!["a@x.com", "b@y.com", "c@z.com"]
        );
        assert!(parse_addrs("   ").is_empty());
        assert!(parse_addrs("").is_empty());
    }

    #[test]
    fn build_settings_validates_and_defaults() {
        // valid, with the insecure flag passed through
        let (email, s) = super::build_settings("me@x.com", "imap.x", "993", "me", true).unwrap();
        assert_eq!(email, "me@x.com");
        assert!(s.allow_invalid_certs);
        assert_eq!(s.port, 993);
        // empty port defaults to 993
        assert_eq!(
            super::build_settings("me@x.com", "h", "", "u", false)
                .unwrap()
                .1
                .port,
            993
        );
        // rejects empties + bad ports
        assert!(super::build_settings("", "h", "993", "u", false).is_err());
        assert!(super::build_settings("me@x.com", "", "993", "u", false).is_err());
        assert!(super::build_settings("me@x.com", "h", "0", "u", false).is_err());
        assert!(super::build_settings("me@x.com", "h", "notaport", "u", false).is_err());
    }

    #[test]
    fn build_smtp_defaults_by_security() {
        use geleit_store::SmtpSecurityKind;
        assert_eq!(
            super::build_smtp_settings("smtp.x", "", false)
                .unwrap()
                .port,
            465
        );
        assert_eq!(
            super::build_smtp_settings("smtp.x", "", true).unwrap().port,
            587
        );
        assert_eq!(
            super::build_smtp_settings("smtp.x", "", true)
                .unwrap()
                .security,
            SmtpSecurityKind::StartTls
        );
        assert!(super::build_smtp_settings("", "587", true).is_err());
        assert!(super::build_smtp_settings("smtp.x", "0", true).is_err());
    }
}

// The live (`dangerous-tls`) test's imports are gated with it to avoid unused-import warnings in the
// default build.
#[cfg(all(test, feature = "dangerous-tls"))]
mod tests {
    use super::*;
    use geleit_platform::secret::InMemorySecretStore;
    use geleit_store::ImapSettings;

    /// The durable flag-write-back queue reaches the server (SYNC-5), end to end against Dovecot.
    ///
    /// A read made **here** marks the message dirty (queued); `run_flush_flags` pushes it to the
    /// server and clears the marker. This is what makes a change survive a failed first attempt — the
    /// scheduler retries the queue every sweep. Simulated by marking read locally (no immediate push)
    /// then draining the queue, and confirming the server's `\Seen` from a fresh connection.
    #[test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    fn the_flag_queue_pushes_a_local_change_to_the_server_and_clears_it() {
        let path = std::env::temp_dir().join(format!("geleit-syncq-{}.db", std::process::id()));
        let path = path.to_str().unwrap();
        let _ = std::fs::remove_file(path);
        let secrets = InMemorySecretStore::new();
        let (email, imap) = build_settings(
            "geleittest@localhost",
            "127.0.0.1",
            "993",
            "geleittest",
            true,
        )
        .unwrap();
        let smtp = build_smtp_settings("127.0.0.1", "465", false).unwrap();
        let acc = run_setup(
            path,
            &secrets,
            &email,
            Some("geleittest"),
            imap,
            smtp,
            "",
            "testpass123",
        )
        .expect("setup + first sync");

        let cfg = imap::ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        let folder = format!("GeleitQueue{}", std::process::id());
        let rt = runtime().unwrap();
        rt.block_on(async {
            let _ = imap::delete_folder(&cfg, &secrets, &folder).await;
            imap::create_folder(&cfg, &secrets, &folder).await.unwrap();
            imap::append_message(
                &cfg,
                &secrets,
                &folder,
                "()",
                b"Subject: unread\r\nFrom: A <a@example.com>\r\n\r\nBody.\r\n",
            )
            .await
            .unwrap();
        });

        let store = open_store(path, &secrets).unwrap();
        rt.block_on(imap::sync_folder_incremental(
            &cfg, &secrets, &store, acc, &folder, 50,
        ))
        .expect("sync the new folder");
        let folder_id = store
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .find(|f| f.name == folder)
            .unwrap()
            .id;
        let id = store.messages_in_folder(folder_id, 10).unwrap()[0].id;

        // Read it HERE, local-only (as open_message does) → queued, server still says unread.
        store.set_seen(id, true).unwrap();
        assert_eq!(
            store.pending_flag_writebacks(acc).unwrap().len(),
            1,
            "queued"
        );
        drop(store);

        // Drain the queue — the change goes to the server, and the marker clears.
        let confirmed = run_flush_flags(path, &secrets, acc).expect("flush");
        assert_eq!(confirmed, 1);
        let store = open_store(path, &secrets).unwrap();
        assert!(
            store.pending_flag_writebacks(acc).unwrap().is_empty(),
            "the queue is drained once the server confirms"
        );
        drop(store);

        // The server really is `\Seen` now — read it back from a fresh connection.
        let server_seen: std::collections::HashSet<u32> = rt.block_on(async {
            let mut sess = imap::connect(&cfg, &secrets).await.unwrap();
            sess.select(&folder).await.unwrap();
            let s = sess.uid_search("SEEN").await.unwrap();
            let _ = sess.logout().await;
            s.into_iter().collect()
        });
        let uid = store_uid(path, &secrets, folder_id);
        assert!(server_seen.contains(&uid), "the read reached the server");

        // A second flush is a no-op (nothing owed).
        assert_eq!(run_flush_flags(path, &secrets, acc).unwrap(), 0);
        rt.block_on(async {
            let _ = imap::delete_folder(&cfg, &secrets, &folder).await;
        });
        let _ = std::fs::remove_file(path);
    }

    /// A queued flag change for a message the server has since **deleted** must not wedge the queue
    /// (SYNC-5): its `STORE` is a harmless no-op on an RFC 3501 server, so the flush counts it done.
    #[test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    fn the_flag_queue_does_not_get_stuck_on_a_message_deleted_on_the_server() {
        let path = std::env::temp_dir().join(format!("geleit-syncqd-{}.db", std::process::id()));
        let path = path.to_str().unwrap();
        let _ = std::fs::remove_file(path);
        let secrets = InMemorySecretStore::new();
        let (email, imap) = build_settings(
            "geleittest@localhost",
            "127.0.0.1",
            "993",
            "geleittest",
            true,
        )
        .unwrap();
        let smtp = build_smtp_settings("127.0.0.1", "465", false).unwrap();
        let acc = run_setup(
            path,
            &secrets,
            &email,
            Some("geleittest"),
            imap,
            smtp,
            "",
            "testpass123",
        )
        .expect("setup");
        let cfg = imap::ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        let folder = format!("GeleitQueueDel{}", std::process::id());
        let rt = runtime().unwrap();
        rt.block_on(async {
            let _ = imap::delete_folder(&cfg, &secrets, &folder).await;
            imap::create_folder(&cfg, &secrets, &folder).await.unwrap();
            imap::append_message(
                &cfg,
                &secrets,
                &folder,
                "()",
                b"Subject: doomed\r\nFrom: A <a@example.com>\r\n\r\nBody.\r\n",
            )
            .await
            .unwrap();
        });
        let store = open_store(path, &secrets).unwrap();
        rt.block_on(imap::sync_folder_incremental(
            &cfg, &secrets, &store, acc, &folder, 50,
        ))
        .unwrap();
        let folder_id = store
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .find(|f| f.name == folder)
            .unwrap()
            .id;
        let msg = &store.messages_in_folder(folder_id, 10).unwrap()[0];
        let (id, uid) = (msg.id, msg.uid.unwrap() as u32);
        store.set_seen(id, true).unwrap(); // queue a read for it…
        drop(store);

        // …then delete it on the server, so its UID is gone before the flush runs.
        rt.block_on(imap::delete_permanently(&cfg, &secrets, &folder, uid))
            .unwrap();

        // The flush must count it done and drain the queue, not retry it forever.
        assert_eq!(run_flush_flags(path, &secrets, acc).expect("flush"), 1);
        let store = open_store(path, &secrets).unwrap();
        assert!(
            store.pending_flag_writebacks(acc).unwrap().is_empty(),
            "a deleted message does not wedge the queue"
        );
        drop(store);
        rt.block_on(async {
            let _ = imap::delete_folder(&cfg, &secrets, &folder).await;
        });
        let _ = std::fs::remove_file(path);
    }

    fn store_uid(path: &str, secrets: &InMemorySecretStore, folder_id: i64) -> u32 {
        let store = open_store(path, secrets).unwrap();
        store.messages_in_folder(folder_id, 10).unwrap()[0]
            .uid
            .unwrap() as u32
    }

    /// The exact refresh + backfill path the Tauri `refresh` command drives (minus the event
    /// wrapper, which only forwards the `on_batch` count), against a local Dovecot. Proves the S9.3
    /// safety net actually pulls mail and streams progress.
    /// The full `run_setup` path the Tauri `add_account` command drives (S9.6), against Dovecot:
    /// validate → create → store the password → first sync. Reads the mailbox back with `open_store`
    /// (SQLCipher-aware) — the old Slint test used `Store::open` and had been broken since
    /// encryption-at-rest landed (it opened the encrypted DB unencrypted).
    #[test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    fn live_setup_creates_and_syncs_an_account() {
        let path = std::env::temp_dir().join(format!("geleit-s96-setup-{}.db", std::process::id()));
        let path = path.to_str().unwrap();
        let _ = std::fs::remove_file(path);

        let secrets = InMemorySecretStore::new();
        let (email, imap) = build_settings(
            "geleittest@localhost",
            "127.0.0.1",
            "993",
            "geleittest",
            true,
        )
        .expect("valid imap form");
        let smtp = build_smtp_settings("127.0.0.1", "465", false).expect("valid smtp form");

        let acc = run_setup(
            path,
            &secrets,
            &email,
            Some("geleittest"),
            imap,
            smtp,
            "",
            "testpass123",
        )
        .expect("setup + first sync");

        // Read back through the ENCRYPTED store (the bug the old Slint test hid).
        let store = open_store(path, &secrets).expect("reopen encrypted");
        let inbox = store
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .find(|f| f.name == "INBOX")
            .expect("INBOX synced")
            .id;
        assert!(!store.messages_in_folder(inbox, 10).unwrap().is_empty());
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    fn live_refresh_then_backfill_streams_progress() {
        let path =
            std::env::temp_dir().join(format!("geleit-s94-refresh-{}.db", std::process::id()));
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
