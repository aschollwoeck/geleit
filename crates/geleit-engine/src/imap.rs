//! IMAP connectivity — connect to one account over TLS, log in, and list folders (ACC-3,
//! READ-6). Async (`tokio` + `async-imap`), TLS via `rustls`/`ring` (ADR-0006). Credentials come
//! from the platform [`SecretStore`] seam (SEC-2) and are **never logged** (constitution P2).

use std::sync::Arc;

use futures::StreamExt;
use geleit_platform::secret::{SecretError, SecretStore};
use geleit_store::{NewMessage, Store, StoreError};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/// Manual IMAP connection config (ACC-3). The password is **not** held here — it is fetched from
/// the [`SecretStore`] at connect time.
#[derive(Clone)]
pub struct ImapConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    /// Dev-only: accept ANY server certificate — this disables authentication entirely (an
    /// active MITM is undetectable), so it offers no protection, only encryption. Only honoured
    /// when the crate is built with the `dangerous-tls` feature (absent from release/CI builds);
    /// otherwise requesting it errors. For the local self-signed Dovecot only.
    pub allow_invalid_certs: bool,
}

/// Errors from IMAP operations. No variant carries credentials or message content (P2).
#[derive(Debug, Error)]
pub enum ImapError {
    #[error("no usable password stored for this account")]
    NoPassword,
    #[error("server sent no greeting")]
    NoGreeting,
    #[error("invalid server name")]
    InvalidServerName,
    #[error("insecure TLS (allow_invalid_certs) is not enabled in this build")]
    InsecureTlsUnavailable,
    #[error("network error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),
    #[error("IMAP protocol error: {0}")]
    Imap(#[from] async_imap::error::Error),
    #[error("secret store error: {0}")]
    Secret(#[from] SecretError),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
}

/// The `service` key under which IMAP passwords are stored in the [`SecretStore`].
const SECRET_SERVICE: &str = "geleit-imap";

/// A logged-in IMAP session over the TLS stream.
type ImapSession = async_imap::Session<tokio_rustls::client::TlsStream<TcpStream>>;

/// Open a TLS connection and log in, returning a ready session. The password comes from the
/// `SecretStore` seam and is never logged (P2).
async fn connect(config: &ImapConfig, secrets: &dyn SecretStore) -> Result<ImapSession, ImapError> {
    // Fetch the password first: missing → fail before opening any socket.
    // NOTE: `SecretStore::get` is sync and the real backend (OsSecretStore, S2.1) makes a blocking
    // D-Bus call. It's safe today — `run_setup`/`run_refresh` drive this on a dedicated worker
    // runtime, never the UI executor — but if connect() is ever called on a shared async executor,
    // move this behind `spawn_blocking` (guidelines §5).
    let password = secrets
        .get(SECRET_SERVICE, &config.username)?
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or(ImapError::NoPassword)?;

    let tcp = TcpStream::connect((config.host.as_str(), config.port)).await?;
    let connector = TlsConnector::from(Arc::new(tls_config(config.allow_invalid_certs)?));
    let server_name =
        ServerName::try_from(config.host.clone()).map_err(|_| ImapError::InvalidServerName)?;
    // `async-imap` (runtime-tokio) speaks tokio's I/O traits, so the tokio-rustls stream is passed
    // directly — no futures/compat wrapper.
    let tls = connector.connect(server_name, tcp).await?;

    let mut client = async_imap::Client::new(tls);
    let _greeting = client.read_response().await?.ok_or(ImapError::NoGreeting)?;

    client
        .login(&config.username, &password)
        .await
        .map_err(|(err, _client)| ImapError::from(err))
}

/// Connect, list folders, and log out. Returns the folder names.
pub async fn list_folders(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
) -> Result<Vec<String>, ImapError> {
    let mut session = connect(config, secrets).await?;
    let mut folders = Vec::new();
    {
        let mut names = session.list(Some(""), Some("*")).await?;
        while let Some(name) = names.next().await {
            folders.push(name?.name().to_string());
        }
    }
    let _ = session.logout().await; // best-effort: we already have the folders
    Ok(folders)
}

/// Join a list of envelope addresses into comma-separated bare addr-specs (for reply-all storage).
fn join_addrs(list: Option<&Vec<async_imap::imap_proto::types::Address>>) -> Option<String> {
    let addrs: Vec<String> = list?
        .iter()
        .filter_map(|a| {
            crate::envelope::address_parts(
                a.name.as_deref(),
                a.mailbox.as_deref(),
                a.host.as_deref(),
            )
            .1
        })
        .collect();
    (!addrs.is_empty()).then(|| addrs.join(", "))
}

/// Map an IMAP FETCH result to a storable envelope. Network-side (the pure decode/format bits live
/// in [`crate::envelope`]). `has_attachments`/`snippet` need the body (S1.6), so are left empty.
fn fetch_to_new_message(f: &async_imap::types::Fetch) -> NewMessage {
    let env = f.envelope();
    let (from_name, from_addr) = env
        .and_then(|e| e.from.as_ref())
        .and_then(|addrs| addrs.first())
        .map(|a| {
            crate::envelope::address_parts(
                a.name.as_deref(),
                a.mailbox.as_deref(),
                a.host.as_deref(),
            )
        })
        .unwrap_or((None, None));
    NewMessage {
        uid: f.uid.map(i64::from),
        message_id: env.and_then(|e| crate::envelope::decode_header(e.message_id.as_deref())),
        in_reply_to: env.and_then(|e| crate::envelope::decode_header(e.in_reply_to.as_deref())),
        subject: env.and_then(|e| crate::envelope::decode_header(e.subject.as_deref())),
        from_name,
        from_addr,
        to_addrs: env.and_then(|e| join_addrs(e.to.as_ref())),
        cc_addrs: env.and_then(|e| join_addrs(e.cc.as_ref())),
        date: f.internal_date().map(|d| d.timestamp()),
        seen: f
            .flags()
            .any(|fl| matches!(fl, async_imap::types::Flag::Seen)),
        flagged: f
            .flags()
            .any(|fl| matches!(fl, async_imap::types::Flag::Flagged)),
        has_attachments: false,
        snippet: None,
    }
}

/// Fetch a folder's most recent envelopes (up to `limit`) and store them; returns how many were
/// fetched. Naive — a recent window, not incremental (CONDSTORE/QRESYNC is M2).
pub async fn sync_envelopes(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    store: &Store,
    account_id: i64,
    folder: &str,
    limit: u32,
) -> Result<usize, ImapError> {
    let folder_id = store.upsert_folder(account_id, folder)?;
    let mut session = connect(config, secrets).await?;
    let mailbox = session.select(folder).await?;
    let mut count = 0usize;
    // NOTE: `store.upsert_message` is a synchronous SQLite write on the async path (as with
    // `SecretStore::get` in `connect`). When driven from the UI it should run via `spawn_blocking`
    // or a store actor; rusqlite's `Connection` is `!Sync`, so this future is `!Send` today
    // (guidelines §5) — the integration slice will address it.
    if let Some((start, end)) = crate::envelope::recent_window(mailbox.exists, limit) {
        // The data items MUST be parenthesised for a multi-item FETCH (IMAP grammar).
        let query = "(UID ENVELOPE FLAGS INTERNALDATE)";
        let mut fetches = session.fetch(format!("{start}:{end}"), query).await?;
        while let Some(fetch) = fetches.next().await {
            let msg = fetch_to_new_message(&fetch?);
            // Skip messages with no UID: they can't be de-duplicated on re-sync, so persisting
            // them would create duplicates (P6). RFC 3501 requires UID when it is requested.
            if msg.uid.is_none() {
                continue;
            }
            store.upsert_message(account_id, folder_id, &msg)?;
            count += 1;
        }
    }
    let _ = session.logout().await; // best-effort
    Ok(count)
}

/// Store an IMAP password for `username` in the secret seam (under the IMAP service key), so
/// callers needn't know the internal service name. The password is never logged (P2).
pub fn store_password(
    secrets: &dyn SecretStore,
    username: &str,
    password: &[u8],
) -> Result<(), ImapError> {
    secrets.set(SECRET_SERVICE, username, password)?;
    Ok(())
}

/// Whether a password is currently available for `username` (e.g. set this session). Lets the UI
/// prompt for a re-entry after a restart without attempting a doomed connection.
pub fn has_password(secrets: &dyn SecretStore, username: &str) -> Result<bool, ImapError> {
    Ok(secrets.get(SECRET_SERVICE, username)?.is_some())
}

/// Read the stored password for `username` (shared with SMTP — same credentials). `None` if absent.
/// The caller must not log it (P2).
pub fn password(secrets: &dyn SecretStore, username: &str) -> Result<Option<Vec<u8>>, ImapError> {
    Ok(secrets.get(SECRET_SERVICE, username)?)
}

/// Add or remove an IMAP system flag (e.g. `\Flagged`, `\Seen`) on a message by UID in `folder` —
/// the shared `UID STORE +/-FLAGS` write-back. `flag` must be a valid flag token (we only pass
/// constants).
async fn store_flag(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
    uid: u32,
    add: bool,
    flag: &str,
) -> Result<(), ImapError> {
    let mut session = connect(config, secrets).await?;
    session.select(folder).await?;
    let op = if add { "+FLAGS" } else { "-FLAGS" };
    let result = drain(
        session
            .uid_store(uid.to_string(), format!("{op} ({flag})"))
            .await,
    )
    .await;
    let _ = session.logout().await; // best-effort
    result
}

/// Set or clear the `\Flagged` (star) flag on a message by UID in `folder` (ORG-4 write-back).
pub async fn set_flag(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
    uid: u32,
    flagged: bool,
) -> Result<(), ImapError> {
    store_flag(config, secrets, folder, uid, flagged, "\\Flagged").await
}

/// Set or clear the `\Seen` (read) flag on a message by UID in `folder` (SYNC-5 read write-back).
pub async fn set_seen(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
    uid: u32,
    seen: bool,
) -> Result<(), ImapError> {
    store_flag(config, secrets, folder, uid, seen, "\\Seen").await
}

/// Move a message by UID from `source` to `target` (ORG-1/2/3 write-back; archive/trash/move all
/// reduce to a move). Uses the IMAP `MOVE` extension (`UID MOVE`); the target mailbox must exist.
pub async fn move_message(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    source: &str,
    uid: u32,
    target: &str,
) -> Result<(), ImapError> {
    let mut session = connect(config, secrets).await?;
    session.select(source).await?;
    let result = session.uid_mv(uid.to_string(), target).await;
    let _ = session.logout().await; // best-effort
    result?;
    Ok(())
}

/// Permanently delete one message by UID (ORG-2): mark `\Deleted` then `UID EXPUNGE`.
pub async fn delete_permanently(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
    uid: u32,
) -> Result<(), ImapError> {
    let mut session = connect(config, secrets).await?;
    session.select(folder).await?;
    let res = match drain(
        session
            .uid_store(uid.to_string(), "+FLAGS (\\Deleted)")
            .await,
    )
    .await
    {
        Ok(()) => drain(session.uid_expunge(uid.to_string()).await).await,
        Err(e) => Err(e),
    };
    let _ = session.logout().await; // best-effort
    res
}

/// Empty a folder (ORG-2, empty-trash): mark every message `\Deleted` then `EXPUNGE`.
pub async fn empty_folder(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
) -> Result<(), ImapError> {
    let mut session = connect(config, secrets).await?;
    let mailbox = session.select(folder).await?;
    let res = if mailbox.exists > 0 {
        match drain(session.store("1:*", "+FLAGS (\\Deleted)").await).await {
            Ok(()) => drain(session.expunge().await).await,
            Err(e) => Err(e),
        }
    } else {
        Ok(())
    };
    let _ = session.logout().await; // best-effort
    res
}

/// Fetch one message's full raw RFC 822 bytes by UID (READ-8, save an attachment on demand). Uses
/// `BODY.PEEK[]` so the fetch doesn't set `\Seen`. `Ok(None)` if the message isn't there. The whole
/// message is fetched (not a single part) — it reuses the sync path's plumbing and messages are
/// already fetched whole at sync time, so there's no new per-part machinery to maintain.
pub async fn fetch_raw_message(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
    uid: u32,
) -> Result<Option<Vec<u8>>, ImapError> {
    let mut session = connect(config, secrets).await?;
    session.select(folder).await?;
    let result = fetch_first_body(&mut session, uid).await;
    let _ = session.logout().await; // best-effort
    result
}

/// The first `BODY[]` payload for a UID fetch (there's at most one), or `None`.
async fn fetch_first_body(
    session: &mut ImapSession,
    uid: u32,
) -> Result<Option<Vec<u8>>, ImapError> {
    let mut fetches = session.uid_fetch(uid.to_string(), "(BODY.PEEK[])").await?;
    while let Some(fetch) = fetches.next().await {
        let fetch = fetch?;
        if let Some(body) = fetch.body() {
            return Ok(Some(body.to_vec()));
        }
    }
    Ok(None)
}

/// Consume an IMAP response stream (e.g. STORE's FETCH replies) to completion, surfacing the first
/// error. We don't need the per-message data, only that the command succeeded.
async fn drain<S, T>(stream: Result<S, async_imap::error::Error>) -> Result<(), ImapError>
where
    S: futures::Stream<Item = Result<T, async_imap::error::Error>>,
{
    let stream = stream?;
    futures::pin_mut!(stream); // expunge/uid_expunge streams are !Unpin
    while let Some(item) = stream.next().await {
        item?;
    }
    Ok(())
}

/// Append a (sent) message to `folder` — e.g. saving an outgoing message to Sent (SEND-8) — marked
/// `\Seen`. `message` is the full RFC 5322 bytes. The mailbox must already exist on the server.
pub async fn append_message(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
    message: &[u8],
) -> Result<(), ImapError> {
    let mut session = connect(config, secrets).await?;
    let result = session
        .append(folder, Some("(\\Seen)"), None, message)
        .await;
    let _ = session.logout().await; // best-effort
    result?;
    Ok(())
}

/// Remove a stored IMAP password (e.g. on account removal, SEC-3). Idempotent — deleting an absent
/// secret succeeds (the `SecretStore` contract).
pub fn delete_password(secrets: &dyn SecretStore, username: &str) -> Result<(), ImapError> {
    secrets.delete(SECRET_SERVICE, username)?;
    Ok(())
}

/// Fetch full bodies for a folder's recent window, MIME-parse them, and store each body (matched
/// to its already-synced message by UID; run [`sync_envelopes`] first). Returns how many bodies
/// were stored. `BODY.PEEK[]` is used so reading a body here does not set `\Seen`.
pub async fn sync_bodies(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    store: &Store,
    account_id: i64,
    folder: &str,
    limit: u32,
) -> Result<usize, ImapError> {
    let folder_id = store.upsert_folder(account_id, folder)?;
    let mut session = connect(config, secrets).await?;
    let mailbox = session.select(folder).await?;
    let mut count = 0usize;
    // NOTE (guidelines §5): both `mime::parse_body` (CPU-bound) and `store.*` (sync SQLite) run on
    // the async executor thread here; the integration slice should move them behind `spawn_blocking`
    // / a store actor, and add a max-body-size guard before parsing (whole body held in memory).
    if let Some((start, end)) = crate::envelope::recent_window(mailbox.exists, limit) {
        let mut fetches = session
            .fetch(format!("{start}:{end}"), "(UID BODY.PEEK[])")
            .await?;
        while let Some(fetch) = fetches.next().await {
            let fetch = fetch?;
            let (Some(uid), Some(raw)) = (fetch.uid.map(i64::from), fetch.body()) else {
                continue; // need both a UID (to match) and a body section
            };
            let Some(message_id) = store.message_id_by_uid(account_id, folder_id, uid)? else {
                continue; // envelope not synced yet — skip (sync_envelopes first)
            };
            if store.body_for(message_id)?.is_some() {
                continue; // already have this body — don't re-download/re-parse
            }
            let parsed = crate::mime::parse_body(raw);
            store.store_body(
                message_id,
                parsed.plain.as_deref(),
                parsed.html.as_deref(),
                parsed.snippet.as_deref(),
                parsed.has_attachments,
            )?;
            count += 1;
        }
    }
    let _ = session.logout().await; // best-effort
    Ok(count)
}

/// Incrementally sync one folder: reconcile local vs. server UIDs, delete what's gone on the
/// server, and fetch envelopes+bodies for **new** UIDs (the most-recent `limit`; older backfill is
/// S2.4). A UIDVALIDITY change clears the folder first (the server's UIDs are no longer valid).
/// Returns how many new messages were stored. (Server→local flag changes are M6, with write-back.)
pub async fn sync_folder_incremental(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    store: &Store,
    account_id: i64,
    folder: &str,
    limit: u32,
) -> Result<usize, ImapError> {
    let folder_id = store.upsert_folder(account_id, folder)?;
    let mut session = connect(config, secrets).await?;
    let mailbox = session.select(folder).await?;

    // UIDVALIDITY: if it changed since last sync, our stored UIDs are meaningless — drop them.
    if let Some(validity) = mailbox.uid_validity {
        let validity = i64::from(validity);
        if matches!(store.folder_uidvalidity(folder_id)?, Some(prev) if prev != validity) {
            store.clear_folder(folder_id)?;
        }
        store.set_folder_uidvalidity(folder_id, validity)?;
    }

    // Reconcile local vs. the server's current UID set.
    let server: Vec<u32> = session.uid_search("ALL").await?.into_iter().collect();
    let local: Vec<u32> = store
        .uids_in_folder(folder_id)?
        .into_iter()
        .map(|u| u as u32)
        .collect();
    let plan = crate::sync::reconcile(&local, &server);

    // Remove messages deleted on the server.
    let deleted: Vec<i64> = plan.deleted.iter().map(|&u| i64::from(u)).collect();
    store.delete_messages_by_uid(folder_id, &deleted)?;

    // Fetch the most-recent `limit` new UIDs (older backfill is S2.4).
    let mut new_uids = plan.new;
    new_uids.sort_unstable();
    let recent_new = &new_uids[new_uids.len().saturating_sub(limit as usize)..];
    if !recent_new.is_empty() {
        fetch_envelopes_for(
            &mut session,
            store,
            account_id,
            folder_id,
            &uid_set(recent_new),
        )
        .await?;
    }
    // Bodies for any recent message still lacking one — covers the just-fetched envelopes AND
    // retries a body fetch an earlier sync left incomplete, so it self-heals (P6).
    let need_bodies = store.uids_without_body(folder_id, limit)?;
    if !need_bodies.is_empty() {
        fetch_bodies_for(
            &mut session,
            store,
            account_id,
            folder_id,
            &uid_set(&need_bodies),
        )
        .await?;
    }

    let _ = session.logout().await; // best-effort
    Ok(recent_new.len())
}

/// Progressively fetch the rest of a folder, newest-first, in `batch_size` chunks (envelopes+bodies
/// per chunk). `on_batch` is called with the running total after each chunk. Resumable — each chunk
/// commits, so a restart continues from local state. Returns the total fetched (0 if up to date).
pub async fn backfill_folder(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    store: &Store,
    account_id: i64,
    folder: &str,
    batch_size: u32,
    on_batch: &mut dyn FnMut(usize),
) -> Result<usize, ImapError> {
    let folder_id = store.upsert_folder(account_id, folder)?;
    let mut session = connect(config, secrets).await?;
    let _mailbox = session.select(folder).await?;
    // UIDVALIDITY is the incremental path's responsibility; if it changed in the gap before this
    // runs, backfill may transiently refetch under new UIDs, and the next incremental sync (which
    // checks UIDVALIDITY) clears + re-syncs the folder, healing it.

    let server: Vec<u32> = session.uid_search("ALL").await?.into_iter().collect();
    let local: Vec<u32> = store
        .uids_in_folder(folder_id)?
        .into_iter()
        .map(|u| u as u32)
        .collect();
    let mut missing = crate::sync::reconcile(&local, &server).new;
    missing.sort_unstable();
    missing.reverse(); // newest (highest UID) first

    let mut total = 0usize;
    for chunk in missing.chunks(batch_size.max(1) as usize) {
        let set = uid_set(chunk);
        fetch_envelopes_for(&mut session, store, account_id, folder_id, &set).await?;
        fetch_bodies_for(&mut session, store, account_id, folder_id, &set).await?;
        total += chunk.len();
        on_batch(total);
    }

    let _ = session.logout().await; // best-effort
    Ok(total)
}

/// Build an IMAP UID set string ("u1,u2,...") from UIDs of any integer type.
fn uid_set<T: ToString>(uids: &[T]) -> String {
    uids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

// NOTE (guidelines §5): these two helpers are the shared sync-write / MIME-parse choke points for
// every sync path. They run synchronous SQLite writes (and `parse_body`, CPU-bound) on the async
// executor, and hold a whole message body in memory per fetch. Safe today — all callers drive them
// on a dedicated worker runtime, never the UI executor — but a future shared executor should move
// the writes/parse behind `spawn_blocking` and add a max-body-size guard before parsing.

/// `uid_fetch` envelopes for `uid_set` and upsert them (UID-less rows skipped — they can't be
/// de-duplicated on re-sync, P6).
async fn fetch_envelopes_for(
    session: &mut ImapSession,
    store: &Store,
    account_id: i64,
    folder_id: i64,
    uid_set: &str,
) -> Result<(), ImapError> {
    let mut fetches = session
        .uid_fetch(uid_set, "(UID ENVELOPE FLAGS INTERNALDATE)")
        .await?;
    while let Some(fetch) = fetches.next().await {
        let msg = fetch_to_new_message(&fetch?);
        if msg.uid.is_none() {
            continue;
        }
        store.upsert_message(account_id, folder_id, &msg)?;
    }
    Ok(())
}

/// `uid_fetch` bodies for `uid_set` (BODY.PEEK[] — doesn't set `\Seen`), parse, and store them by UID.
async fn fetch_bodies_for(
    session: &mut ImapSession,
    store: &Store,
    account_id: i64,
    folder_id: i64,
    uid_set: &str,
) -> Result<(), ImapError> {
    let mut fetches = session.uid_fetch(uid_set, "(UID BODY.PEEK[])").await?;
    while let Some(fetch) = fetches.next().await {
        let fetch = fetch?;
        let (Some(uid), Some(raw)) = (fetch.uid.map(i64::from), fetch.body()) else {
            continue;
        };
        let Some(message_id) = store.message_id_by_uid(account_id, folder_id, uid)? else {
            continue;
        };
        let parsed = crate::mime::parse_body(raw);
        store.store_body(
            message_id,
            parsed.plain.as_deref(),
            parsed.html.as_deref(),
            parsed.snippet.as_deref(),
            parsed.has_attachments,
        )?;
        let attachments: Vec<geleit_store::Attachment> = parsed
            .attachments
            .iter()
            .map(|a| geleit_store::Attachment {
                filename: a.filename.clone(),
                content_type: a.content_type.clone(),
                size: a.size as i64,
            })
            .collect();
        store.store_attachments(message_id, &attachments)?;
    }
    Ok(())
}

/// Upsert the given folder names into the store under `account_id` (idempotent). Pure — no network.
pub fn persist_folders(
    store: &Store,
    account_id: i64,
    folders: &[String],
) -> Result<(), StoreError> {
    for name in folders {
        store.upsert_folder(account_id, name)?;
    }
    // Reconcile: drop local folders the server no longer lists (rename/delete, ORG-6).
    store.prune_folders(account_id, folders)?;
    Ok(())
}

/// Create a server folder (ORG-6).
pub async fn create_folder(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    name: &str,
) -> Result<(), ImapError> {
    let mut session = connect(config, secrets).await?;
    let result = session.create(name).await;
    let _ = session.logout().await; // best-effort
    result?;
    Ok(())
}

/// Rename a server folder (ORG-6).
pub async fn rename_folder(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    from: &str,
    to: &str,
) -> Result<(), ImapError> {
    let mut session = connect(config, secrets).await?;
    let result = session.rename(from, to).await;
    let _ = session.logout().await; // best-effort
    result?;
    Ok(())
}

/// Delete a server folder (ORG-6).
pub async fn delete_folder(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    name: &str,
) -> Result<(), ImapError> {
    let mut session = connect(config, secrets).await?;
    let result = session.delete(name).await;
    let _ = session.logout().await; // best-effort
    result?;
    Ok(())
}

/// List the account's folders from the server and persist them to the local store.
pub async fn sync_folders(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    store: &Store,
    account_id: i64,
) -> Result<(), ImapError> {
    let folders = list_folders(config, secrets).await?;
    persist_folders(store, account_id, &folders)?;
    Ok(())
}

/// Build a rustls client config. By default it authenticates the server against the Mozilla CA
/// roots. `allow_invalid_certs` (which disables authentication entirely) is only available with
/// the `dangerous-tls` build feature; otherwise it returns [`ImapError::InsecureTlsUnavailable`],
/// so a release/CI build can never silently skip certificate validation.
fn tls_config(allow_invalid_certs: bool) -> Result<ClientConfig, ImapError> {
    // Install the ring crypto provider as the process default once (idempotent).
    let _ = rustls::crypto::ring::default_provider().install_default();

    if allow_invalid_certs {
        #[cfg(feature = "dangerous-tls")]
        {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            return Ok(ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(danger::AcceptAnyServerCert(provider)))
                .with_no_client_auth());
        }
        #[cfg(not(feature = "dangerous-tls"))]
        return Err(ImapError::InsecureTlsUnavailable);
    }

    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Ok(ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth())
}

#[cfg(feature = "dangerous-tls")]
mod danger {
    //! Dev-only certificate verifier. It accepts ANY certificate: chain/name validation is
    //! skipped, so this provides **no authentication and no MITM protection** — only encryption.
    //! (Handshake signatures are still checked, but against the attacker's own key in a MITM, so
    //! that adds nothing.) Compiled only with `dangerous-tls`, for the local self-signed Dovecot.
    use std::sync::Arc;

    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, Error, SignatureScheme};

    #[derive(Debug)]
    pub(super) struct AcceptAnyServerCert(pub Arc<CryptoProvider>);

    impl ServerCertVerifier for AcceptAnyServerCert {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            verify_tls12_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            verify_tls13_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geleit_platform::secret::InMemorySecretStore;

    #[tokio::test]
    async fn missing_password_errors_without_connecting() {
        // No password in the store ⇒ NoPassword before any socket is opened (port is unused).
        let cfg = ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 1,
            username: "nobody".to_owned(),
            allow_invalid_certs: true,
        };
        let secrets = InMemorySecretStore::new();
        assert!(matches!(
            list_folders(&cfg, &secrets).await,
            Err(ImapError::NoPassword)
        ));
    }

    #[test]
    fn persist_folders_is_idempotent_and_scoped() {
        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("a@example.com", None).unwrap();
        persist_folders(&store, acc, &["INBOX".to_owned(), "Sent".to_owned()]).unwrap();
        // re-sync with an extra folder: existing ones are no-ops, new one is added
        persist_folders(
            &store,
            acc,
            &["INBOX".to_owned(), "Sent".to_owned(), "Archive".to_owned()],
        )
        .unwrap();
        assert_eq!(store.folders_for_account(acc).unwrap().len(), 3);
    }

    /// Live test against the local Dovecot (`geleittest`/`testpass123`). Needs the `dangerous-tls`
    /// feature (self-signed cert) and a running server; ignored in CI. Run with:
    /// `cargo test -p geleit-engine --features dangerous-tls -- --ignored`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn live_list_folders_against_dovecot() {
        let secrets = InMemorySecretStore::new();
        secrets
            .set(SECRET_SERVICE, "geleittest", b"testpass123")
            .unwrap();
        let cfg = ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        let folders = list_folders(&cfg, &secrets).await.expect("connect + list");
        assert!(folders.iter().any(|f| f == "INBOX"), "folders: {folders:?}");
    }

    /// Append a message to INBOX (proxy for Sent) and confirm it's accepted (SEND-8).
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn live_append_message_to_dovecot() {
        let secrets = InMemorySecretStore::new();
        secrets
            .set(SECRET_SERVICE, "geleittest", b"testpass123")
            .unwrap();
        let cfg = ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        let msg =
            b"From: geleittest@localhost\r\nTo: x@localhost\r\nSubject: Append test\r\n\r\nHi.\r\n";
        append_message(&cfg, &secrets, "INBOX", msg)
            .await
            .expect("append accepted");
    }

    /// Append a known message to INBOX, sync envelopes, and assert it lands in the store.
    /// Needs `--features dangerous-tls` + a running Dovecot; ignored in CI.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn live_sync_envelopes_from_dovecot() {
        let secrets = InMemorySecretStore::new();
        secrets
            .set(SECRET_SERVICE, "geleittest", b"testpass123")
            .unwrap();
        let cfg = ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        let subject = "Geleit S1.5 envelope test";
        let raw = format!(
            "Subject: {subject}\r\nFrom: Tester <tester@example.com>\r\n\
             Date: Tue, 01 Jul 2026 09:00:00 +0000\r\n\r\nhello\r\n"
        );
        {
            let mut session = connect(&cfg, &secrets).await.expect("connect");
            session
                .append("INBOX", None, None, raw.as_bytes())
                .await
                .expect("append");
            let _ = session.logout().await;
        }

        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("geleittest@localhost", None).unwrap();
        let n = sync_envelopes(&cfg, &secrets, &store, acc, "INBOX", 50)
            .await
            .expect("sync");
        assert!(n >= 1, "synced {n} messages");
        let folder_id = store.upsert_folder(acc, "INBOX").unwrap();
        let msgs = store.messages_in_folder(folder_id, 50).unwrap();
        assert!(
            msgs.iter().any(|m| m.subject.as_deref() == Some(subject)),
            "subjects: {:?}",
            msgs.iter().map(|m| m.subject.clone()).collect::<Vec<_>>()
        );
    }

    /// Append a multipart message (plaintext + attachment), sync envelopes then bodies, and assert
    /// the body, snippet, and attachment flag are stored. Needs `--features dangerous-tls` + Dovecot.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn live_sync_bodies_from_dovecot() {
        let secrets = InMemorySecretStore::new();
        secrets
            .set(SECRET_SERVICE, "geleittest", b"testpass123")
            .unwrap();
        let cfg = ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        let subject = "Geleit S1.6 body test";
        let raw = format!(
            "Subject: {subject}\r\nFrom: Tester <tester@example.com>\r\n\
             MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"B\"\r\n\r\n\
             --B\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nBody in plain text.\r\n\
             --B\r\nContent-Type: text/plain; name=\"a.txt\"\r\n\
             Content-Disposition: attachment; filename=\"a.txt\"\r\n\r\nfile\r\n--B--\r\n"
        );
        {
            let mut session = connect(&cfg, &secrets).await.expect("connect");
            session
                .append("INBOX", None, None, raw.as_bytes())
                .await
                .expect("append");
            let _ = session.logout().await;
        }

        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("geleittest@localhost", None).unwrap();
        sync_envelopes(&cfg, &secrets, &store, acc, "INBOX", 50)
            .await
            .expect("sync envelopes");
        let n = sync_bodies(&cfg, &secrets, &store, acc, "INBOX", 50)
            .await
            .expect("sync bodies");
        assert!(n >= 1, "stored {n} bodies");

        let folder_id = store.upsert_folder(acc, "INBOX").unwrap();
        let msgs = store.messages_in_folder(folder_id, 50).unwrap();
        let m = msgs
            .iter()
            .find(|m| m.subject.as_deref() == Some(subject))
            .expect("message present");
        assert!(m.has_attachments, "expected attachment flag");
        let body = store.body_for(m.id).unwrap().expect("body stored");
        assert!(body.plain.unwrap().contains("Body in plain text"));
    }

    /// Fetch a message's raw bytes on demand and extract an attachment (READ-8, save-to-disk path):
    /// append a multipart message, fetch it whole by UID, and assert the attachment's decoded bytes.
    /// Needs Dovecot + `--features dangerous-tls`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn live_fetch_raw_and_extract_attachment() {
        let secrets = InMemorySecretStore::new();
        secrets
            .set(SECRET_SERVICE, "geleittest", b"testpass123")
            .unwrap();
        let cfg = ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        let subject = "Geleit READ-8 attachment fetch";
        let raw = format!(
            "Subject: {subject}\r\nFrom: Tester <tester@example.com>\r\n\
             MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"B\"\r\n\r\n\
             --B\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nSee attached.\r\n\
             --B\r\nContent-Type: text/plain; name=\"report.txt\"\r\n\
             Content-Disposition: attachment; filename=\"report.txt\"\r\n\r\n\
             the attachment bytes\r\n--B--\r\n"
        );
        // Append and find its UID.
        let uid = {
            let mut session = connect(&cfg, &secrets).await.expect("connect");
            session
                .append("INBOX", None, None, raw.as_bytes())
                .await
                .expect("append");
            session.select("INBOX").await.expect("select");
            let uids = session
                .uid_search(format!("SUBJECT \"{subject}\""))
                .await
                .expect("search");
            let uid = *uids.iter().max().expect("a uid");
            let _ = session.logout().await;
            uid
        };

        // Fetch the whole raw message on demand, then extract attachment index 0.
        let fetched = fetch_raw_message(&cfg, &secrets, "INBOX", uid)
            .await
            .expect("fetch")
            .expect("a message body");
        let att = crate::mime::extract_attachment(&fetched, 0).expect("attachment 0");
        assert_eq!(att.filename.as_deref(), Some("report.txt"));
        assert_eq!(att.data, b"the attachment bytes");
    }

    /// Folder management round-trip (ORG-6): create a folder, confirm it lists, rename it, confirm the
    /// new name lists (old gone), then delete it and confirm it's gone. Needs Dovecot + dangerous-tls.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn live_create_rename_delete_folder() {
        let secrets = InMemorySecretStore::new();
        secrets
            .set(SECRET_SERVICE, "geleittest", b"testpass123")
            .unwrap();
        let cfg = ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        // Clean up any leftover from a previous run (best-effort).
        let _ = delete_folder(&cfg, &secrets, "GeleitTmpA").await;
        let _ = delete_folder(&cfg, &secrets, "GeleitTmpB").await;

        create_folder(&cfg, &secrets, "GeleitTmpA")
            .await
            .expect("create");
        let after_create = list_folders(&cfg, &secrets).await.expect("list");
        assert!(
            after_create.iter().any(|f| f == "GeleitTmpA"),
            "created folder should list: {after_create:?}"
        );

        rename_folder(&cfg, &secrets, "GeleitTmpA", "GeleitTmpB")
            .await
            .expect("rename");
        let after_rename = list_folders(&cfg, &secrets).await.expect("list");
        assert!(after_rename.iter().any(|f| f == "GeleitTmpB"), "renamed");
        assert!(!after_rename.iter().any(|f| f == "GeleitTmpA"), "old gone");

        delete_folder(&cfg, &secrets, "GeleitTmpB")
            .await
            .expect("delete");
        let after_delete = list_folders(&cfg, &secrets).await.expect("list");
        assert!(!after_delete.iter().any(|f| f == "GeleitTmpB"), "deleted");
    }

    /// Incremental sync: a new message appears, a re-sync is idempotent (no dupes), and a message
    /// deleted on the server is removed locally. Needs Dovecot + `--features dangerous-tls`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn live_incremental_new_and_delete() {
        use futures::StreamExt;

        let secrets = InMemorySecretStore::new();
        secrets
            .set(SECRET_SERVICE, "geleittest", b"testpass123")
            .unwrap();
        let cfg = ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        let subject = "Geleit S2.3 incremental test";
        let raw =
            format!("Subject: {subject}\r\nFrom: T <t@example.com>\r\n\r\nincremental body\r\n");
        {
            let mut session = connect(&cfg, &secrets).await.expect("connect");
            session
                .append("INBOX", None, None, raw.as_bytes())
                .await
                .expect("append");
            let _ = session.logout().await;
        }

        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("geleittest@localhost", None).unwrap();
        sync_folder_incremental(&cfg, &secrets, &store, acc, "INBOX", 200)
            .await
            .expect("sync 1");
        let folder_id = store.upsert_folder(acc, "INBOX").unwrap();
        let present = |s: &Store| {
            s.messages_in_folder(folder_id, 500)
                .unwrap()
                .into_iter()
                .filter(|m| m.subject.as_deref() == Some(subject))
                .collect::<Vec<_>>()
        };
        let found = present(&store);
        assert_eq!(found.len(), 1, "new message appears");
        let uid = found[0].uid.expect("uid");

        // re-sync is idempotent (no duplicate)
        sync_folder_incremental(&cfg, &secrets, &store, acc, "INBOX", 200)
            .await
            .expect("sync 2");
        assert_eq!(present(&store).len(), 1, "no duplicate on re-sync");

        // delete on the server, then sync → gone locally
        {
            let mut session = connect(&cfg, &secrets).await.expect("connect");
            session.select("INBOX").await.expect("select");
            let mut upd = session
                .uid_store(format!("{uid}"), "+FLAGS (\\Deleted)")
                .await
                .expect("store flags");
            while upd.next().await.is_some() {}
            drop(upd);
            {
                let ex = session.expunge().await.expect("expunge"); // !Unpin → pin to iterate
                futures::pin_mut!(ex);
                while ex.next().await.is_some() {}
            }
            let _ = session.logout().await;
        }
        sync_folder_incremental(&cfg, &secrets, &store, acc, "INBOX", 200)
            .await
            .expect("sync 3");
        assert!(
            present(&store).is_empty(),
            "deleted message removed locally"
        );
    }

    /// A synced reply has its `In-Reply-To` stored (the link threading needs). Needs Dovecot +
    /// `--features dangerous-tls`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn live_in_reply_to_stored() {
        let secrets = InMemorySecretStore::new();
        secrets
            .set(SECRET_SERVICE, "geleittest", b"testpass123")
            .unwrap();
        let cfg = ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        let subject = "Geleit S3.4 reply test";
        let raw = format!(
            "Subject: {subject}\r\nFrom: T <t@example.com>\r\nMessage-ID: <reply-s34@x>\r\n\
             In-Reply-To: <parent-s34@x>\r\n\r\na reply\r\n"
        );
        {
            let mut session = connect(&cfg, &secrets).await.expect("connect");
            session
                .append("INBOX", None, None, raw.as_bytes())
                .await
                .expect("append");
            let _ = session.logout().await;
        }
        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("geleittest@localhost", None).unwrap();
        sync_folder_incremental(&cfg, &secrets, &store, acc, "INBOX", 50)
            .await
            .expect("sync");
        let folder_id = store.upsert_folder(acc, "INBOX").unwrap();
        let m = store
            .messages_in_folder(folder_id, 50)
            .unwrap()
            .into_iter()
            .find(|m| m.subject.as_deref() == Some(subject))
            .expect("message present");
        assert_eq!(m.in_reply_to.as_deref(), Some("<parent-s34@x>"));
    }

    /// A synced message with an attachment has its attachment metadata stored. Needs Dovecot +
    /// `--features dangerous-tls`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn live_attachment_metadata_stored() {
        let secrets = InMemorySecretStore::new();
        secrets
            .set(SECRET_SERVICE, "geleittest", b"testpass123")
            .unwrap();
        let cfg = ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        let subject = "Geleit S3.5 attachment test";
        let raw = format!(
            "Subject: {subject}\r\nFrom: T <t@example.com>\r\nMIME-Version: 1.0\r\n\
             Content-Type: multipart/mixed; boundary=\"B\"\r\n\r\n\
             --B\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nsee attached\r\n\
             --B\r\nContent-Type: text/plain; name=\"report.txt\"\r\n\
             Content-Disposition: attachment; filename=\"report.txt\"\r\n\r\nthe report body\r\n--B--\r\n"
        );
        {
            let mut session = connect(&cfg, &secrets).await.expect("connect");
            session
                .append("INBOX", None, None, raw.as_bytes())
                .await
                .expect("append");
            let _ = session.logout().await;
        }

        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("geleittest@localhost", None).unwrap();
        sync_folder_incremental(&cfg, &secrets, &store, acc, "INBOX", 50)
            .await
            .expect("sync");
        let folder_id = store.upsert_folder(acc, "INBOX").unwrap();
        let m = store
            .messages_in_folder(folder_id, 50)
            .unwrap()
            .into_iter()
            .find(|m| m.subject.as_deref() == Some(subject))
            .expect("message present");
        let atts = store.attachments_for(m.id).unwrap();
        assert!(
            atts.iter()
                .any(|a| a.filename.as_deref() == Some("report.txt") && a.size > 0),
            "attachment metadata stored: {atts:?}"
        );
    }

    /// Backfill fetches messages beyond the incremental cap, newest-first, with monotonic progress,
    /// and is idempotent once complete. Needs Dovecot + `--features dangerous-tls`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn live_backfill_fetches_the_rest() {
        let secrets = InMemorySecretStore::new();
        secrets
            .set(SECRET_SERVICE, "geleittest", b"testpass123")
            .unwrap();
        let cfg = ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        let subjects: Vec<String> = (0..4)
            .map(|i| format!("Geleit S2.4 backfill {i}"))
            .collect();
        {
            let mut session = connect(&cfg, &secrets).await.expect("connect");
            for s in &subjects {
                let raw = format!("Subject: {s}\r\nFrom: T <t@example.com>\r\n\r\nbody {s}\r\n");
                session
                    .append("INBOX", None, None, raw.as_bytes())
                    .await
                    .expect("append");
            }
            let _ = session.logout().await;
        }

        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("geleittest@localhost", None).unwrap();
        // recent window only
        sync_folder_incremental(&cfg, &secrets, &store, acc, "INBOX", 2)
            .await
            .expect("incremental");
        let folder_id = store.upsert_folder(acc, "INBOX").unwrap();

        // backfill the rest, batched
        let mut totals = Vec::new();
        let fetched = backfill_folder(&cfg, &secrets, &store, acc, "INBOX", 2, &mut |n| {
            totals.push(n)
        })
        .await
        .expect("backfill");
        assert!(fetched > 0, "backfill fetched some");
        assert!(
            totals.windows(2).all(|w| w[0] < w[1]),
            "monotonic: {totals:?}"
        );
        assert_eq!(*totals.last().unwrap(), fetched);

        // every appended message is now present with a body
        let msgs = store.messages_in_folder(folder_id, 1000).unwrap();
        for s in &subjects {
            let m = msgs
                .iter()
                .find(|m| m.subject.as_deref() == Some(s.as_str()))
                .unwrap_or_else(|| panic!("missing {s}"));
            assert!(store.body_for(m.id).unwrap().is_some(), "body for {s}");
        }

        // already complete → no-op
        let mut again_totals = Vec::new();
        let again = backfill_folder(&cfg, &secrets, &store, acc, "INBOX", 2, &mut |n| {
            again_totals.push(n)
        })
        .await
        .expect("backfill 2");
        assert_eq!(again, 0);
        assert!(again_totals.is_empty());
    }
}
