//! IMAP connectivity — connect to one account over TLS, log in, and list folders (ACC-3,
//! READ-6). Async (`tokio` + `async-imap`), TLS via `rustls`/`ring` (ADR-0006). Credentials come
//! from the platform [`SecretStore`] seam (SEC-2) and are **never logged** (constitution P2).

use std::sync::Arc;

use crate::sync::{news_for_backfill, owed, Arrived, News};
use async_imap::types::NameAttribute;
use futures::StreamExt;
use geleit_core::FolderRole;
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
    #[error("the server doesn't support IMAP IDLE")]
    IdleUnsupported,
}

/// What one incremental sync of a folder turned up (NOTIF-1).
#[derive(Debug, Clone, Default)]
pub struct SyncOutcome {
    /// Messages that were not in our store before this sync, with what a notification would need.
    pub arrived: Vec<Arrived>,
    /// Whether the folder was in a known-good state *before* this sync. When false, `arrived` is the
    /// whole recent window rather than genuine news — see [`crate::sync::notifiable`].
    pub primed: bool,
    /// How many already-held messages had their read/star flags changed by this sync — i.e. read or
    /// starred on another device (SYNC-5). Non-zero means the on-screen list is now stale even though
    /// no mail *arrived*, so the UI should re-list and the badge should be recomputed.
    pub flag_updates: usize,
}

impl SyncOutcome {
    /// The arrivals worth telling the user about — unseen, and only from a primed folder.
    #[must_use]
    pub fn worth_announcing(&self) -> Vec<&Arrived> {
        crate::sync::notifiable(&self.arrived, self.primed)
    }
}

/// The `service` key under which IMAP passwords are stored in the [`SecretStore`].
const SECRET_SERVICE: &str = "geleit-imap";

/// How many UIDs go in one `UID FETCH` for a raw-body export (see [`collect_raw_bodies`]) — small enough
/// that the command line can't hit a server/proxy length limit, large enough to keep round-trips down.
const RAW_FETCH_CHUNK: usize = 256;

/// How long to wait for an export's connect (TCP + TLS + login) before giving up and degrading to
/// reconstruction — bounds an offline export instead of letting it hang on the OS TCP timeout.
const CONNECT_TIMEOUT_SECS: u64 = 15;

/// A logged-in IMAP session over the TLS stream.
type ImapSession = async_imap::Session<tokio_rustls::client::TlsStream<TcpStream>>;

/// Open a TLS connection and log in, returning a ready session. The password comes from the
/// `SecretStore` seam and is never logged (P2).
pub(crate) async fn connect(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
) -> Result<ImapSession, ImapError> {
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

/// A folder as the server presents it: its name, and what the server says it is **for**.
pub type FolderListing = (String, Option<FolderRole>);

/// What role the server declared for a folder, from its LIST attributes (RFC 6154 SPECIAL-USE). Pure.
///
/// This is the whole point of the feature: the *name* of a folder is in the user's language, but
/// `\Drafts` is not.
///
/// A mailbox may carry **several** special uses (RFC 6154 §2; Dovecot's `special_use` takes a list), so
/// the choice goes through [`geleit_core::pick_role`], which applies a fixed priority — otherwise
/// `(\Sent \Archive)` and `(\Archive \Sent)` would name different folders for "where sent mail goes",
/// depending on nothing but the order the server felt like sending.
///
/// `\All` (Gmail's "All Mail") is deliberately **not** an archive: archiving there is a no-op — every
/// message is already in it — so treating it as one would make Archive silently do nothing.
/// `\Flagged` is a saved search, not a folder we ever move mail into.
#[must_use]
pub fn special_use_role(attributes: &[NameAttribute<'_>]) -> Option<FolderRole> {
    let roles: Vec<FolderRole> = attributes
        .iter()
        .filter_map(|a| match a {
            NameAttribute::Drafts => Some(FolderRole::Drafts),
            NameAttribute::Sent => Some(FolderRole::Sent),
            NameAttribute::Trash => Some(FolderRole::Trash),
            NameAttribute::Junk => Some(FolderRole::Junk),
            NameAttribute::Archive => Some(FolderRole::Archive),
            _ => None,
        })
        .collect();
    geleit_core::pick_role(&roles)
}

/// Watch the INBOX for new mail with IMAP IDLE (RFC 2177), calling `on_activity` the instant the server
/// pushes something — so mail is noticed in seconds, not on the next poll.
///
/// Holds **one** connection: it re-IDLEs on the 28-minute timeout (under RFC 2177's 29-minute limit)
/// rather than reconnecting, and only returns on a connection error (so the caller can reconnect with
/// backoff) or [`ImapError::IdleUnsupported`] (so the caller stops trying IDLE and leans on polling).
/// `on_activity` is deliberately tiny — it just wakes the existing sync scheduler, which does the
/// actual fetch/notify/badge — so IDLE is a low-latency *trigger*, not a second sync path.
pub async fn idle_watch(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
    on_activity: &(dyn Fn() + Send + Sync),
) -> Result<std::convert::Infallible, ImapError> {
    use async_imap::extensions::idle::IdleResponse;
    use std::time::Duration;

    // Every command has a ceiling, so a **half-open** connection (a slept laptop, a NAT that dropped the
    // flow without a RST) surfaces as an error and the caller reconnects — rather than the read blocking
    // forever and the watcher hanging silently. A long-lived IDLE connection meets this far more often
    // than the short per-op connections elsewhere.
    const OP_TIMEOUT: Duration = Duration::from_secs(60);
    // Re-IDLE on this **wall clock**, under RFC 2177's 29-minute cap. It has to be an outer timer, not
    // the library's `wait_with_timeout`: that one resets on every server keepalive (`* OK Still here`),
    // so on a chatty server it would never elapse and we'd never re-IDLE.
    const REIDLE: Duration = Duration::from_secs(28 * 60);

    let timed = |label: &'static str| {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("IMAP {label} timed out"),
        )
    };

    let mut session = connect(config, secrets).await?;
    let caps = tokio::time::timeout(OP_TIMEOUT, session.capabilities())
        .await
        .map_err(|_| timed("CAPABILITY"))??;
    if !caps.has_str("IDLE") {
        let _ = session.logout().await;
        return Err(ImapError::IdleUnsupported);
    }
    tokio::time::timeout(OP_TIMEOUT, session.select(folder))
        .await
        .map_err(|_| timed("SELECT"))??;

    loop {
        let mut handle = session.idle();
        tokio::time::timeout(OP_TIMEOUT, handle.init())
            .await
            .map_err(|_| timed("IDLE"))??;

        // Race the server push against our own wall-clock re-IDLE timer.
        let (idle_fut, _stop) = handle.wait_with_timeout(REIDLE);
        let response = match tokio::time::timeout(REIDLE, idle_fut).await {
            Ok(res) => res?,
            Err(_) => IdleResponse::Timeout, // wall-clock elapsed → re-IDLE
        };
        // Close IDLE cleanly and take the session back for the next round — bounded, so a dead
        // connection can't hang here either.
        session = tokio::time::timeout(OP_TIMEOUT, handle.done())
            .await
            .map_err(|_| timed("DONE"))??;

        match response {
            // The server pushed something — new mail, a flag change. Wake the scheduler to sync now.
            IdleResponse::NewData(_) => on_activity(),
            // Nothing to report (a timeout / our re-IDLE) — just IDLE again on the same connection.
            IdleResponse::Timeout | IdleResponse::ManualInterrupt => {}
        }
    }
}

/// Connect, list folders, and log out. Returns each folder's name and the role the server gave it.
pub async fn list_folders(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
) -> Result<Vec<FolderListing>, ImapError> {
    let mut session = connect(config, secrets).await?;
    let mut folders = Vec::new();
    {
        let mut names = session.list(Some(""), Some("*")).await?;
        while let Some(name) = names.next().await {
            let name = name?;
            let role = special_use_role(name.attributes());
            // IMAP reserves the name INBOX itself (RFC 3501), and servers rarely bother to flag it.
            let role = role.or_else(|| {
                name.name()
                    .eq_ignore_ascii_case("inbox")
                    .then_some(FolderRole::Inbox)
            });
            folders.push((name.name().to_string(), role));
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
        // The caller decides — see `sync::owed` — because only it knows whether this folder had ever
        // been looked at, and whether this UID is new mail or the old mail a backfill exists to fetch.
        owed_notification: false,
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

/// Push a batch of read/star states to the server in **one session** (SYNC-5 durable write-back).
///
/// For each `(uid, seen, flagged)` it brings the server's `\Seen` and `\Flagged` for that message into
/// line with what we hold — `+FLAGS`/`-FLAGS` per flag, so **other** flags (`\Answered`, `\Draft`, …)
/// are never touched. Returns the UIDs that were pushed successfully, so the caller clears the dirty
/// marker for exactly those and leaves the rest to the next sweep. A message no longer on the server is
/// counted as done (its `STORE` is a harmless no-op) rather than blocking the queue forever.
pub async fn push_flags(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
    items: &[(u32, bool, bool)],
) -> Result<Vec<u32>, ImapError> {
    let mut session = connect(config, secrets).await?;
    session.select(folder).await?;
    let mut pushed = Vec::new();
    for &(uid, seen, flagged) in items {
        let seen_op = if seen {
            "+FLAGS (\\Seen)"
        } else {
            "-FLAGS (\\Seen)"
        };
        let flag_op = if flagged {
            "+FLAGS (\\Flagged)"
        } else {
            "-FLAGS (\\Flagged)"
        };
        let a = drain(session.uid_store(uid.to_string(), seen_op).await).await;
        let b = drain(session.uid_store(uid.to_string(), flag_op).await).await;
        if a.is_ok() && b.is_ok() {
            pushed.push(uid);
        }
    }
    let _ = session.logout().await; // best-effort
    Ok(pushed)
}

/// Push a batch of queued moves out of one **source** folder (OFF-4), mirroring [`push_flags`]: connect
/// and `SELECT` the source once, then `UID MOVE` each message to its (possibly different) target. Each
/// item is `(uid, target)`.
///
/// The two failure kinds are kept apart, and that split is the whole point:
/// - **Couldn't connect or select the source** → the outer `Err`. The account is unreachable (offline)
///   or the source folder is gone; the caller leaves every move in the batch queued to retry.
/// - **The session is up but a `UID MOVE` is refused** (unknown target, a uid already gone) → that item
///   comes back `false`. Retrying it never helps, so the caller stops hiding it. A move that *landed*
///   comes back `true`.
pub async fn move_batch(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    source: &str,
    items: &[(u32, String)],
) -> Result<Vec<(u32, bool)>, ImapError> {
    let mut session = connect(config, secrets).await?;
    session.select(source).await?;
    let mut results = Vec::with_capacity(items.len());
    for (uid, target) in items {
        let moved = session.uid_mv(uid.to_string(), target).await.is_ok();
        results.push((*uid, moved));
    }
    let _ = session.logout().await; // best-effort
    Ok(results)
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

/// Fetch the **raw original bytes** of many messages in one folder, by UID, in a single session — for a
/// complete export/backup (SEC-4). `BODY.PEEK[]` gets the whole RFC 5322 message *as the server holds
/// it*, attachments and all, without setting `\Seen`. Returns a `uid → bytes` map holding only the ones
/// the server actually returned (a uid that has since vanished is simply absent). Blocking + network:
/// **worker thread.**
pub async fn fetch_raw_batch(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
    uids: &[u32],
) -> Result<std::collections::HashMap<u32, Vec<u8>>, ImapError> {
    if uids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    // Bound the *connect* (TCP + TLS + login) so an unreachable server — a firewall dropping SYNs, not
    // refusing — degrades an export in seconds instead of hanging on the OS TCP timeout (~75-120s). The
    // fetch itself is left unbounded: a big folder legitimately takes a while to download, and cutting
    // that off would silently turn a slow-but-complete export into an incomplete one.
    let mut session = tokio::time::timeout(
        std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS),
        connect(config, secrets),
    )
    .await
    .map_err(|_| ImapError::Io(std::io::Error::from(std::io::ErrorKind::TimedOut)))??;
    session.select(folder).await?;
    let result = collect_raw_bodies(&mut session, uids).await;
    let _ = session.logout().await; // best-effort
    result
}

/// The UID-fetch loop for [`fetch_raw_batch`], split out so the fetch stream (which borrows the session)
/// is dropped before the caller logs out. Asks for `UID` alongside the body so each payload can be keyed
/// back to its message.
///
/// UIDs are fetched in **chunks** rather than one command carrying every uid: a whole folder's worth
/// (`1,2,…,50000`) is a hundreds-of-KB command line that a server or proxy may reject outright — which
/// would fail the entire fetch and silently drop the folder to reconstruction. Chunking keeps each
/// command small and bounded.
async fn collect_raw_bodies(
    session: &mut ImapSession,
    uids: &[u32],
) -> Result<std::collections::HashMap<u32, Vec<u8>>, ImapError> {
    let mut out = std::collections::HashMap::new();
    for chunk in uids.chunks(RAW_FETCH_CHUNK) {
        let set = chunk
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let mut fetches = session.uid_fetch(set, "(UID BODY.PEEK[])").await?;
        while let Some(fetch) = fetches.next().await {
            let fetch = fetch?;
            if let (Some(uid), Some(body)) = (fetch.uid, fetch.body()) {
                out.insert(uid, body.to_vec());
            }
        }
    }
    Ok(out)
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

/// Append a message to `folder` with the given IMAP `flags` (a parenthesised list, e.g. `(\Seen)`
/// for a Sent-save, `(\Draft)` for a draft). `message` is the full RFC 5322 bytes. The mailbox must
/// already exist on the server.
pub async fn append_message(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
    flags: &str,
    message: &[u8],
) -> Result<(), ImapError> {
    let mut session = connect(config, secrets).await?;
    let result = session.append(folder, Some(flags), None, message).await;
    let _ = session.logout().await; // best-effort
    result?;
    Ok(())
}

/// Save a draft's copy to `folder` (SEND-5): expunge whatever copies of this draft are already there
/// (matched by the stable `message_id` we stamp — this also cleans up an orphan a previous failure
/// left behind), then `APPEND` the new bytes flagged `\Draft`. One session for the whole exchange.
///
/// IMAP has no update-in-place, so replace-then-append *is* the edit. `APPENDUID` isn't surfaced by
/// our client, which is exactly why the copy is identified by its Message-ID rather than a UID.
pub async fn sync_draft(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
    message_id: &str,
    bytes: &[u8],
) -> Result<(), ImapError> {
    let mut session = connect(config, secrets).await?;
    let result = async {
        session.select(folder).await?;
        expunge_draft_copies(&mut session, message_id).await?;
        session
            .append(folder, Some("(\\Draft)"), None, bytes)
            .await?;
        Ok(())
    }
    .await;
    let _ = session.logout().await; // best-effort
    result
}

/// Remove a draft's copy from `folder` (SEND-5) — on send, discard, or when the sync setting is
/// turned off. Idempotent: no copy there is success.
pub async fn expunge_draft(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    folder: &str,
    message_id: &str,
) -> Result<(), ImapError> {
    let mut session = connect(config, secrets).await?;
    let result = async {
        session.select(folder).await?;
        expunge_draft_copies(&mut session, message_id).await
    }
    .await;
    let _ = session.logout().await; // best-effort
    result
}

/// Expunge every `\Draft`-flagged message in the selected folder carrying `message_id`. The `DRAFT`
/// search key is a safety belt: even if a Message-ID somehow collided, we can only ever expunge a
/// draft — never real mail.
async fn expunge_draft_copies(
    session: &mut ImapSession,
    message_id: &str,
) -> Result<(), ImapError> {
    // Quote-escape so an odd Message-ID can't break out of the search string.
    let escaped = message_id.replace('\\', "\\\\").replace('"', "\\\"");
    let uids = session
        .uid_search(format!("DRAFT HEADER Message-ID \"{escaped}\""))
        .await?;
    if uids.is_empty() {
        return Ok(());
    }
    let set = uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    drain(session.uid_store(set.clone(), "+FLAGS (\\Deleted)").await).await?;
    drain(session.uid_expunge(set).await).await
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
/// Returns a [`SyncOutcome`]: which messages arrived, and whether the folder was primed (i.e.
/// whether those arrivals are genuinely news — see [`crate::sync::should_announce`]). (Server→local flag changes are M6, with write-back.)
pub async fn sync_folder_incremental(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
    store: &Store,
    account_id: i64,
    folder: &str,
    limit: u32,
) -> Result<SyncOutcome, ImapError> {
    let folder_id = store.upsert_folder(account_id, folder)?;
    let mut session = connect(config, secrets).await?;
    let mailbox = session.select(folder).await?;

    // Has this folder ever completed a sync? Only then does "absent from our store" mean "new mail"
    // rather than "we have never looked" — the whole decision lives in `sync::should_announce`.
    let was_primed = store.folder_primed(folder_id)?;

    // UIDVALIDITY: if it changed since last sync, our stored UIDs are meaningless — drop them. That
    // also makes every message look new again, so the folder must be primed afresh.
    let mut uidvalidity_changed = false;
    // A UIDVALIDITY reset clears the folder and re-fetches it, and the re-fetch announces nothing (the
    // folder is unprimed again — otherwise every message in the inbox would pop up). But a message we
    // owed a notification for and hadn't raised yet — held through quiet hours, say — would be silently
    // written off by that rebuild. The server invalidating its UIDs is not the user being told, so the
    // debt is carried across, by Message-ID.
    let mut owed_across_reset: Vec<String> = Vec::new();
    if let Some(validity) = mailbox.uid_validity {
        let validity = i64::from(validity);
        if matches!(store.folder_uidvalidity(folder_id)?, Some(prev) if prev != validity) {
            owed_across_reset = store.owed_message_ids(folder_id)?;
            store.clear_folder(folder_id)?;
            store.set_folder_primed(folder_id, false)?;
            uidvalidity_changed = true;
        }
        store.set_folder_uidvalidity(folder_id, validity)?;
    }
    let primed = crate::sync::should_announce(was_primed, uidvalidity_changed);

    // Reconcile local vs. the server's current UID set.
    let server: Vec<u32> = session.uid_search("ALL").await?.into_iter().collect();
    let local: Vec<u32> = store
        .uids_in_folder(folder_id)?
        .into_iter()
        .map(|u| u as u32)
        .collect();
    let plan = crate::sync::reconcile(&local, &server);

    // Remove messages deleted on the server FIRST, so the flag pull below neither reconciles nor counts
    // a message that is about to vanish anyway (which would waste a write and spuriously re-list).
    let deleted: Vec<i64> = plan.deleted.iter().map(|&u| i64::from(u)).collect();
    store.delete_messages_by_uid(folder_id, &deleted)?;

    // Pull read/star changes made on ANOTHER device (SYNC-5) for messages we already hold. Two cheap
    // `UID SEARCH`es give the server's `\Seen` / `\Flagged` UID sets whatever the mailbox size — far
    // lighter than re-fetching every message's flags — and `sync::flag_plan` keeps only what actually
    // differs from what we hold. This is what makes "read it on my phone" drop the unread badge here.
    // `flags_in_folder` excludes messages with an unconfirmed local change, so the pull can't undo a
    // read the user just made here whose write-back to the server hasn't landed yet.
    let flag_updates = {
        let server_seen: std::collections::HashSet<u32> =
            session.uid_search("SEEN").await?.into_iter().collect();
        let server_flagged: std::collections::HashSet<u32> =
            session.uid_search("FLAGGED").await?.into_iter().collect();
        let held: Vec<crate::sync::FlagState> = store
            .flags_in_folder(folder_id)?
            .into_iter()
            .map(|(uid, seen, flagged)| crate::sync::FlagState {
                uid: uid as u32,
                seen,
                flagged,
            })
            .collect();
        let changes = crate::sync::flag_plan(&held, &server_seen, &server_flagged);
        let rows: Vec<(i64, bool, bool)> = changes
            .iter()
            .map(|c| (i64::from(c.uid), c.seen, c.flagged))
            .collect();
        store.apply_flag_changes(folder_id, &rows)?;
        rows.len()
    };

    // Fetch the most-recent `limit` new UIDs (older backfill is S2.4).
    let mut new_uids = plan.new;
    new_uids.sort_unstable();
    let recent_new = &new_uids[new_uids.len().saturating_sub(limit as usize)..];
    let mut arrived = Vec::new();
    if !recent_new.is_empty() {
        arrived = fetch_envelopes_for(
            &mut session,
            store,
            account_id,
            folder_id,
            &uid_set(recent_new),
            // An unprimed folder announces nothing: "absent from our store" only means "we have never
            // looked", and a new account would otherwise notify once per message in its inbox.
            if primed { News::All } else { News::None },
        )
        .await?;
    }
    // Bodies for any recent message still lacking one — covers the just-fetched envelopes AND
    // retries a body fetch an earlier sync left incomplete, so it self-heals (P6).
    //
    // Deliberately **best-effort**: the envelopes are already committed, so failing here with `?`
    // would throw away `arrived` — and those UIDs are now local, so no later sync would ever call
    // them new again. The mail would sit in the inbox, silently, never announced. A missing body is
    // the far smaller problem, and `uids_without_body` retries it on the next sync anyway.
    let need_bodies = store.uids_without_body(folder_id, limit)?;
    if !need_bodies.is_empty() {
        let _ = fetch_bodies_for(
            &mut session,
            store,
            account_id,
            folder_id,
            &uid_set(&need_bodies),
        )
        .await;
    }

    // Mail we already owed the user, that the UIDVALIDITY rebuild has just re-fetched as though it
    // were old: it is still owed. (Anything they read elsewhere in the meantime stays settled.)
    if !owed_across_reset.is_empty() {
        store.restore_owed(folder_id, &owed_across_reset)?;
    }

    // The folder has now completed a sync — from here on, "absent from our store" really does mean
    // new mail. Set even when the folder was empty, so the FIRST message into an empty inbox is
    // announced (inferring primed-ness from "has messages" would have swallowed it).
    store.set_folder_primed(folder_id, true)?;

    let _ = session.logout().await; // best-effort
    Ok(SyncOutcome {
        arrived,
        primed,
        flag_updates,
    })
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

    // The backfill exists to fetch **old** mail, and old mail is not news. But it can also sweep up a
    // message that arrived while it was running — and that message would then be in our store, so no
    // later sync would ever call it new. That is precisely the mail the old diff-based signal lost, so
    // anything above the newest UID we already held is still owed a notification.
    let news = news_for_backfill(&local);

    let mut total = 0usize;
    for chunk in missing.chunks(batch_size.max(1) as usize) {
        let set = uid_set(chunk);
        fetch_envelopes_for(&mut session, store, account_id, folder_id, &set, news).await?;
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
    news: News,
) -> Result<Vec<Arrived>, ImapError> {
    let mut fetches = session
        .uid_fetch(uid_set, "(UID ENVELOPE FLAGS INTERNALDATE)")
        .await?;
    let mut arrived = Vec::new();
    while let Some(fetch) = fetches.next().await {
        let mut msg = fetch_to_new_message(&fetch?);
        let Some(uid) = msg.uid else {
            continue;
        };
        // Whether we owe the user a notification is written **with the message**, so it survives
        // whichever sync path happened to store it (migration 17). The decision itself is pure and
        // lives in `sync::owed`.
        msg.owed_notification = owed(news, uid as u32, msg.seen);
        // These UIDs came from the reconcile plan, so each one really is new to us. Record what a
        // notification would need before the envelope is consumed by the store.
        arrived.push(Arrived {
            uid: uid as u32,
            from: crate::envelope::display_sender(
                msg.from_name.as_deref(),
                msg.from_addr.as_deref(),
            ),
            subject: msg.subject.clone().unwrap_or_default(),
            seen: msg.seen,
        });
        store.upsert_message(account_id, folder_id, &msg)?;
    }
    Ok(arrived)
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

/// Upsert the listed folders — names **and the roles the server gave them** — under `account_id`
/// (idempotent). Pure — no network.
///
/// The role is rewritten every time, because the server owns it: mark a different folder as Drafts in
/// webmail and the next listing must move the role with it.
pub fn persist_folders(
    store: &Store,
    account_id: i64,
    folders: &[FolderListing],
) -> Result<(), StoreError> {
    for (name, role) in folders {
        store.upsert_folder_with_role(account_id, name, role.map(FolderRole::key))?;
    }
    // Reconcile: drop local folders the server no longer lists (rename/delete, ORG-6).
    let names: Vec<String> = folders.iter().map(|(n, _)| n.clone()).collect();
    store.prune_folders(account_id, &names)?;
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
        let listing = |names: &[(&str, Option<FolderRole>)]| -> Vec<super::FolderListing> {
            names.iter().map(|(n, r)| ((*n).to_owned(), *r)).collect()
        };
        persist_folders(
            &store,
            acc,
            &listing(&[("INBOX", Some(FolderRole::Inbox)), ("Sent", None)]),
        )
        .unwrap();
        // re-sync with an extra folder: existing ones are no-ops, new one is added
        persist_folders(
            &store,
            acc,
            &listing(&[
                ("INBOX", Some(FolderRole::Inbox)),
                ("Sent", None),
                ("Archive", None),
            ]),
        )
        .unwrap();
        assert_eq!(store.folders_for_account(acc).unwrap().len(), 3);
    }

    #[test]
    fn a_folders_role_is_the_servers_to_change() {
        // The server owns the role: mark a different folder as Drafts in webmail, and the next listing
        // must move the role with it — including *off* the folder that used to hold it. (An account
        // synced before this feature existed has no roles at all until its next listing, which is why
        // the name fallback stays.)
        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("a@example.com", None).unwrap();
        let role_of = |name: &str| {
            store
                .folders_for_account(acc)
                .unwrap()
                .into_iter()
                .find(|f| f.name == name)
                .unwrap()
                .role
        };

        persist_folders(
            &store,
            acc,
            &[
                ("Entwürfe".to_owned(), Some(FolderRole::Drafts)),
                ("Alte Entwürfe".to_owned(), None),
            ],
        )
        .unwrap();
        assert_eq!(role_of("Entwürfe").as_deref(), Some("drafts"));
        assert_eq!(role_of("Alte Entwürfe"), None);

        persist_folders(
            &store,
            acc,
            &[
                ("Entwürfe".to_owned(), None),
                ("Alte Entwürfe".to_owned(), Some(FolderRole::Drafts)),
            ],
        )
        .unwrap();
        assert_eq!(
            role_of("Entwürfe"),
            None,
            "the role moved away with the flag"
        );
        assert_eq!(role_of("Alte Entwürfe").as_deref(), Some("drafts"));
    }

    #[test]
    fn syncing_a_folders_mail_never_blanks_the_role_the_listing_gave_it() {
        // `upsert_folder` is what every *message* sync calls — it knows a name and nothing else. If it
        // wrote a NULL role, the first sync after a listing would wipe the roles and the app would fall
        // back to English names again, silently.
        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("a@example.com", None).unwrap();
        persist_folders(
            &store,
            acc,
            &[("Papierkorb".to_owned(), Some(FolderRole::Trash))],
        )
        .unwrap();
        store.upsert_folder(acc, "Papierkorb").unwrap();
        assert_eq!(
            store.folders_for_account(acc).unwrap()[0].role.as_deref(),
            Some("trash")
        );
    }

    #[test]
    fn the_special_use_attributes_are_read_off_the_listing() {
        use async_imap::types::NameAttribute as A;
        // What a real LIST line looks like: housekeeping attributes alongside the special use.
        assert_eq!(
            super::special_use_role(&[A::NoInferiors, A::Unmarked, A::Drafts]),
            Some(FolderRole::Drafts)
        );
        assert_eq!(super::special_use_role(&[A::Sent]), Some(FolderRole::Sent));
        assert_eq!(
            super::special_use_role(&[A::Trash]),
            Some(FolderRole::Trash)
        );
        assert_eq!(super::special_use_role(&[A::Junk]), Some(FolderRole::Junk));
        assert_eq!(
            super::special_use_role(&[A::Archive]),
            Some(FolderRole::Archive)
        );
        // An ordinary folder has no role…
        assert_eq!(super::special_use_role(&[A::NoSelect]), None);
        assert_eq!(super::special_use_role(&[]), None);
        // A folder with two special uses resolves the same way whichever order the server sent them —
        // otherwise "where does sent mail go" would depend on nothing but the server's mood.
        assert_eq!(
            super::special_use_role(&[A::Sent, A::Archive]),
            super::special_use_role(&[A::Archive, A::Sent])
        );
        // …and neither `\All` (Gmail's "All Mail": archiving into it is a no-op, since everything is
        // already there) nor `\Flagged` (a saved search) is a folder we may move mail into.
        assert_eq!(super::special_use_role(&[A::All]), None);
        assert_eq!(super::special_use_role(&[A::Flagged]), None);
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
        assert!(
            folders.iter().any(|(n, _)| n == "INBOX"),
            "folders: {folders:?}"
        );
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
        append_message(&cfg, &secrets, "INBOX", "(\\Seen)", msg)
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
            after_create.iter().any(|(n, _)| n == "GeleitTmpA"),
            "created folder should list: {after_create:?}"
        );

        rename_folder(&cfg, &secrets, "GeleitTmpA", "GeleitTmpB")
            .await
            .expect("rename");
        let after_rename = list_folders(&cfg, &secrets).await.expect("list");
        assert!(
            after_rename.iter().any(|(n, _)| n == "GeleitTmpB"),
            "renamed"
        );
        assert!(
            !after_rename.iter().any(|(n, _)| n == "GeleitTmpA"),
            "old gone"
        );

        delete_folder(&cfg, &secrets, "GeleitTmpB")
            .await
            .expect("delete");
        let after_delete = list_folders(&cfg, &secrets).await.expect("list");
        assert!(
            !after_delete.iter().any(|(n, _)| n == "GeleitTmpB"),
            "deleted"
        );
    }

    /// Server-backed drafts (SEND-5, opt-in): append a `\Draft` message to a Drafts folder, find its
    /// UID by the Message-ID we stamped (APPENDUID isn't surfaced), then expunge it — the full
    /// save / re-save / discard lifecycle. Needs Dovecot + `--features dangerous-tls`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn live_append_find_and_expunge_a_server_draft() {
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
        // Dovecot may not have a Drafts folder out of the box — make sure one exists.
        let _ = create_folder(&cfg, &secrets, "Drafts").await;

        let mid = "<geleit-draft-live-test@geleit.local>";
        let draft = |body: &str| {
            format!(
                "Message-ID: {mid}\r\nFrom: geleittest@localhost\r\n\
                 Subject: Geleit SEND-5 server draft\r\n\r\n{body}\r\n"
            )
        };
        // Clean slate, then save the draft to the server.
        expunge_draft(&cfg, &secrets, "Drafts", mid)
            .await
            .expect("clean");
        sync_draft(
            &cfg,
            &secrets,
            "Drafts",
            mid,
            draft("First version.").as_bytes(),
        )
        .await
        .expect("sync");

        // Exactly one copy, flagged \Draft, carrying the first body.
        let one = fetch_draft_bodies(&cfg, &secrets, "Drafts", mid).await;
        assert_eq!(one.len(), 1, "one copy after the first save");
        assert!(one[0].contains("First version."), "body: {}", one[0]);

        // A re-save REPLACES it (IMAP has no update-in-place) — still exactly one copy, new body.
        sync_draft(
            &cfg,
            &secrets,
            "Drafts",
            mid,
            draft("Edited version.").as_bytes(),
        )
        .await
        .expect("re-sync");
        let again = fetch_draft_bodies(&cfg, &secrets, "Drafts", mid).await;
        assert_eq!(again.len(), 1, "a re-save must not leave a duplicate");
        assert!(again[0].contains("Edited version."), "body: {}", again[0]);

        // Sending / discarding removes it, and doing so twice is fine.
        expunge_draft(&cfg, &secrets, "Drafts", mid)
            .await
            .expect("expunge");
        expunge_draft(&cfg, &secrets, "Drafts", mid)
            .await
            .expect("expunge is idempotent");
        assert!(
            fetch_draft_bodies(&cfg, &secrets, "Drafts", mid)
                .await
                .is_empty(),
            "the copy should be gone"
        );
    }

    /// The raw bodies of every `\Draft` message in `folder` carrying `message_id` — proves both that
    /// the copy is there AND that the `\Draft` flag stuck (the search key requires it).
    #[cfg(feature = "dangerous-tls")]
    async fn fetch_draft_bodies(
        cfg: &ImapConfig,
        secrets: &dyn SecretStore,
        folder: &str,
        message_id: &str,
    ) -> Vec<String> {
        let mut session = connect(cfg, secrets).await.expect("connect");
        session.select(folder).await.expect("select");
        let uids = session
            .uid_search(format!("DRAFT HEADER Message-ID \"{message_id}\""))
            .await
            .expect("search");
        let mut out = Vec::new();
        for uid in uids {
            let mut fetches = session
                .uid_fetch(uid.to_string(), "(BODY.PEEK[])")
                .await
                .expect("fetch");
            while let Some(f) = fetches.next().await {
                if let Some(body) = f.expect("fetch row").body() {
                    out.push(String::from_utf8_lossy(body).into_owned());
                }
            }
        }
        let _ = session.logout().await;
        out
    }

    /// Reading (or starring) a message on **another device** reaches this one (SYNC-5), end to end
    /// against a real server.
    ///
    /// The flags a message carries are the server's to change, and a sync now pulls those changes for
    /// mail we already hold — so a message read in webmail stops being unread here, and the badge falls
    /// for it. Simulated by pushing the flag from a *second* connection (what the other device would
    /// do), then syncing and checking the local store followed.
    /// Needs Dovecot + `--features dangerous-tls`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn a_message_read_on_another_device_stops_being_unread_here() {
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
        let folder = format!("GeleitFlags{}", std::process::id());
        let _ = delete_folder(&cfg, &secrets, &folder).await;
        create_folder(&cfg, &secrets, &folder)
            .await
            .expect("create");
        let raw =
            |s: &str| format!("Subject: {s}\r\nFrom: Alice <alice@example.com>\r\n\r\nBody.\r\n");
        for s in ["one", "two"] {
            append_message(&cfg, &secrets, &folder, "()", raw(s).as_bytes())
                .await
                .expect("append unread");
        }

        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("geleittest@localhost", None).unwrap();
        sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("first sync");
        let folder_id = store
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .find(|f| f.name == folder)
            .unwrap()
            .id;
        let unread = || {
            store
                .messages_in_folder(folder_id, 10)
                .unwrap()
                .into_iter()
                .filter(|m| !m.seen)
                .count()
        };
        let starred = || {
            store
                .messages_in_folder(folder_id, 10)
                .unwrap()
                .into_iter()
                .filter(|m| m.flagged)
                .count()
        };
        assert_eq!(unread(), 2, "both arrived unread");
        assert_eq!(starred(), 0);

        // Another device reads the first message and stars the second — pushed straight to the server.
        let uids: Vec<u32> = {
            let mut sess = connect(&cfg, &secrets).await.unwrap();
            sess.select(&folder).await.unwrap();
            let mut u: Vec<u32> = sess.uid_search("ALL").await.unwrap().into_iter().collect();
            let _ = sess.logout().await;
            u.sort_unstable();
            u
        };
        set_seen(&cfg, &secrets, &folder, uids[0], true)
            .await
            .expect("read on device 2");
        set_flag(&cfg, &secrets, &folder, uids[1], true)
            .await
            .expect("star on device 2");

        // Sync again — the change must reach us, and the outcome must SAY it changed so the UI re-lists.
        let outcome = sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("second sync");
        assert_eq!(
            outcome.flag_updates, 2,
            "one read + one starred were pulled"
        );
        assert!(
            outcome.arrived.is_empty(),
            "no mail arrived — only flags moved"
        );
        assert_eq!(
            unread(),
            1,
            "the message read elsewhere is no longer unread here"
        );
        assert_eq!(
            starred(),
            1,
            "the message starred elsewhere is starred here"
        );

        // Idempotent: nothing changed since, so a third sync pulls nothing.
        let third = sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("third sync");
        assert_eq!(
            third.flag_updates, 0,
            "already in sync — no needless writes"
        );

        let _ = delete_folder(&cfg, &secrets, &folder).await;
    }

    /// IDLE notices new mail within seconds (RFC 2177), end to end against Dovecot: watch a folder,
    /// deliver a message from a *second* connection, and the watcher's `on_activity` fires.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn idle_wakes_within_seconds_when_mail_arrives() {
        use std::sync::Arc;
        let secrets = Arc::new(InMemorySecretStore::new());
        secrets
            .set(SECRET_SERVICE, "geleittest", b"testpass123")
            .unwrap();
        let cfg = ImapConfig {
            host: "127.0.0.1".to_owned(),
            port: 993,
            username: "geleittest".to_owned(),
            allow_invalid_certs: true,
        };
        let folder = format!("GeleitIdle{}", std::process::id());
        let _ = delete_folder(&cfg, &*secrets, &folder).await;
        create_folder(&cfg, &*secrets, &folder)
            .await
            .expect("create");

        // A Notify the watcher pokes on activity — the exact shape the app uses (it wakes the scheduler).
        let woke = Arc::new(tokio::sync::Notify::new());
        let woke2 = woke.clone();
        let on_activity = move || woke2.notify_waiters();

        let (cfg2, secrets2, folder2) = (cfg.clone(), Arc::clone(&secrets), folder.clone());
        let watcher = tokio::spawn(async move {
            let _ = idle_watch(&cfg2, &*secrets2, &folder2, &on_activity).await;
        });

        // Give IDLE a moment to establish, then deliver mail from another connection.
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        append_message(
            &cfg,
            &*secrets,
            &folder,
            "()",
            b"Subject: pushed\r\nFrom: A <a@example.com>\r\n\r\nBody.\r\n",
        )
        .await
        .expect("append");

        // The watcher must wake almost immediately — well under the 28-minute re-IDLE, and under the
        // 5-minute poll it exists to beat.
        let first = tokio::time::timeout(std::time::Duration::from_secs(10), woke.notified())
            .await
            .is_ok();
        assert!(first, "IDLE must notice the first message within seconds");

        // …and it must **re-IDLE**: a second message has to wake it too, or the loop stopped after one.
        append_message(
            &cfg,
            &*secrets,
            &folder,
            "()",
            b"Subject: pushed again\r\nFrom: A <a@example.com>\r\n\r\nMore.\r\n",
        )
        .await
        .expect("append 2");
        let second = tokio::time::timeout(std::time::Duration::from_secs(10), woke.notified())
            .await
            .is_ok();
        watcher.abort();
        let _ = delete_folder(&cfg, &*secrets, &folder).await;
        assert!(
            second,
            "IDLE must keep watching — a second message wakes it too (it re-IDLEs)"
        );
    }

    /// A read made **here** that the server hasn't confirmed must NOT be reverted by the flag pull
    /// (the blocker the SYNC-5 review caught).
    ///
    /// Reading a message marks it read locally and writes `\Seen` back on a worker; a sync can run in
    /// between. Here we mark it read **locally only** (as `store.set_seen` does — flags_dirty=1) without
    /// telling the server, then sync: the pull must leave it read, because the change is unconfirmed.
    /// Needs Dovecot + `--features dangerous-tls`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn a_local_read_the_server_has_not_confirmed_is_not_reverted_by_the_pull() {
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
        let folder = format!("GeleitDirty{}", std::process::id());
        let _ = delete_folder(&cfg, &secrets, &folder).await;
        create_folder(&cfg, &secrets, &folder)
            .await
            .expect("create");
        append_message(
            &cfg,
            &secrets,
            &folder,
            "()",
            b"Subject: unread on the server\r\nFrom: Alice <alice@example.com>\r\n\r\nBody.\r\n",
        )
        .await
        .expect("append unread");

        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("geleittest@localhost", None).unwrap();
        sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("first sync");
        let folder_id = store
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .find(|f| f.name == folder)
            .unwrap()
            .id;
        let id = store.messages_in_folder(folder_id, 10).unwrap()[0].id;

        // The user reads it HERE — local only, the server still says unread (the write-back hasn't run,
        // or failed). This is exactly `open_message`'s local step.
        store.set_seen(id, true).unwrap();
        assert!(store.header_by_id(id).unwrap().unwrap().seen);

        // Sync while the server still says unread. The pull must NOT flip the user's read back off.
        let outcome = sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("second sync");
        assert_eq!(
            outcome.flag_updates, 0,
            "the unconfirmed local read is shielded, not reverted"
        );
        assert!(
            store.header_by_id(id).unwrap().unwrap().seen,
            "the read the user made here survives a sync the server hasn't caught up with"
        );

        // Once the write-back confirms (server now agrees), the message is reconciled normally and
        // stays read — no thrash.
        store.clear_flags_dirty(id, true, false).unwrap();
        set_seen(
            &cfg,
            &secrets,
            &folder,
            {
                let mut sess = connect(&cfg, &secrets).await.unwrap();
                sess.select(&folder).await.unwrap();
                let u = sess
                    .uid_search("ALL")
                    .await
                    .unwrap()
                    .into_iter()
                    .next()
                    .unwrap();
                let _ = sess.logout().await;
                u
            },
            true,
        )
        .await
        .expect("the write-back lands");
        let third = sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("third sync");
        assert_eq!(
            third.flag_updates, 0,
            "local and server now agree — nothing to do"
        );
        assert!(store.header_by_id(id).unwrap().unwrap().seen);

        let _ = delete_folder(&cfg, &secrets, &folder).await;
    }

    /// The durable "we owe you a notification" fact (NOTIF-1), end to end against a real server —
    /// including the case it exists for: **a message that arrives while the backfill is running**.
    ///
    /// The old signal was a diff against the store, so whichever writer stored the message first ate
    /// it: the backfill stored the message, no later sync could call it new, and the mail sat in the
    /// inbox unread and unannounced forever. Now the debt is written with the message, and the backfill
    /// only writes off what is genuinely *old* — everything above the newest UID we already held is
    /// still owed.
    /// Needs Dovecot + `--features dangerous-tls`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn mail_the_backfill_sweeps_up_is_still_owed_a_notification() {
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
        // Its own mailbox: sibling tests append to the shared INBOX, and their mail would land in this
        // test's counts.
        let folder = format!("GeleitOwed{}", std::process::id());
        let _ = delete_folder(&cfg, &secrets, &folder).await;
        create_folder(&cfg, &secrets, &folder)
            .await
            .expect("create");
        let raw = |subject: &str| {
            format!("Subject: {subject}\r\nFrom: Alice <alice@example.com>\r\n\r\nBody.\r\n")
        };

        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("geleittest@localhost", None).unwrap();

        // Two messages are already there when we first look — they are not news, however new they are
        // to *us*.
        for s in ["Old one", "Old two"] {
            append_message(&cfg, &secrets, &folder, "()", raw(s).as_bytes())
                .await
                .expect("append");
        }
        sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("first sync");
        let folder_id = store
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .find(|f| f.name == folder)
            .unwrap()
            .id;
        assert!(
            store
                .pending_notifications(folder_id, 10)
                .unwrap()
                .is_empty(),
            "the first sync of a folder announces nothing — it is a first look, not news"
        );

        // Now mail arrives, and it is the BACKFILL that stores it (the case that used to be lost).
        append_message(&cfg, &secrets, &folder, "()", raw("New mail").as_bytes())
            .await
            .expect("append");
        let mut batches = 0;
        backfill_folder(&cfg, &secrets, &store, acc, &folder, 10, &mut |_| {
            batches += 1
        })
        .await
        .expect("backfill");

        let owed = store.pending_notifications(folder_id, 10).unwrap();
        assert_eq!(
            owed.len(),
            1,
            "the backfill stored it — but it is still owed"
        );
        assert_eq!(owed[0].subject.as_deref(), Some("New mail"));
        assert_eq!(owed[0].from_name.as_deref(), Some("Alice"));

        // Telling the user settles it, once. A later sync must not resurrect the debt.
        store.mark_notified(&[owed[0].id]).unwrap();
        sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("re-sync");
        assert!(
            store
                .pending_notifications(folder_id, 10)
                .unwrap()
                .is_empty(),
            "told once, never again"
        );

        // …and a message that is already \Seen on the server was read elsewhere: never news.
        append_message(
            &cfg,
            &secrets,
            &folder,
            "(\\Seen)",
            raw("Read on my phone").as_bytes(),
        )
        .await
        .expect("append seen");
        sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("sync");
        assert!(
            store
                .pending_notifications(folder_id, 10)
                .unwrap()
                .is_empty(),
            "already read elsewhere — announcing it would be telling the user something they know"
        );

        let _ = delete_folder(&cfg, &secrets, &folder).await;
    }

    /// The server tells us what its folders are *for* — and we keep it (RFC 6154 SPECIAL-USE).
    ///
    /// The pure tests build the `NameAttribute`s themselves, so they cannot see whether a real server
    /// sends them or whether `async-imap` surfaces them. This does: Dovecot flags its Drafts folder
    /// `\Drafts`, and after a folder sync the store must say so — that flag is what makes the app work
    /// on a provider whose folders are named in a language we don't read.
    /// Needs Dovecot + `--features dangerous-tls`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn the_server_tells_us_which_folder_is_the_drafts_folder() {
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
        let _ = create_folder(&cfg, &secrets, "Drafts").await; // Dovecot flags this one \Drafts

        let listing = list_folders(&cfg, &secrets).await.expect("list");
        let drafts = listing
            .iter()
            .find(|(name, _)| name.as_str() == "Drafts")
            .expect("a Drafts folder");
        assert_eq!(
            drafts.1,
            Some(FolderRole::Drafts),
            "the server sends \\Drafts on LIST; if this fails, the whole feature is inert"
        );
        // INBOX is the one folder IMAP names itself, and servers rarely flag it.
        let inbox = listing
            .iter()
            .find(|(n, _)| n.as_str() == "INBOX")
            .expect("INBOX");
        assert_eq!(inbox.1, Some(FolderRole::Inbox));

        // …and the role survives into the store, which is where every caller reads it from.
        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("geleittest@localhost", None).unwrap();
        sync_folders(&cfg, &secrets, &store, acc)
            .await
            .expect("sync");
        let stored = store
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .find(|f| f.name == "Drafts")
            .expect("Drafts in the store");
        assert_eq!(stored.role.as_deref(), Some("drafts"));
    }

    /// The hinge of the merged Drafts list: a draft we upload must come back through a **real sync**
    /// carrying the Message-ID we stamped on it, byte for byte.
    ///
    /// Everything else about the merge is pure and unit-tested — but the pure tests build the server
    /// row's Message-ID by calling the same code the dedup calls, so they cannot see this. If a server
    /// (or our own `decode_header`) hands the id back unbracketed, folded, or whitespace-padded, the
    /// dedup silently stops matching and **every synced draft lists twice** — the exact bug the whole
    /// feature exists to kill — with all the unit tests still green.
    /// Needs Dovecot + `--features dangerous-tls`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn a_draft_we_uploaded_comes_back_carrying_the_message_id_we_stamped() {
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
        // Its own mailbox: the shared Drafts folder is used by sibling tests (and by hand).
        let folder = format!("GeleitRoundTrip{}", std::process::id());
        let _ = delete_folder(&cfg, &secrets, &folder).await;
        create_folder(&cfg, &secrets, &folder)
            .await
            .expect("create");

        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("geleittest@localhost", None).unwrap();
        // A draft, with the id the STORE minted for it — the one its copy is appended under.
        let draft_id = store
            .save_draft(
                acc,
                None,
                &geleit_store::DraftContent {
                    subject: "Round trip".to_owned(),
                    body: "Does the id survive?".to_owned(),
                    ..Default::default()
                },
            )
            .unwrap();
        let mid = store.draft_by_id(draft_id).unwrap().unwrap().msgid;

        let raw = format!(
            "Message-ID: {mid}\r\nFrom: geleittest@localhost\r\nSubject: Round trip\r\n\r\nDoes the id survive?\r\n"
        );
        sync_draft(&cfg, &secrets, &folder, &mid, raw.as_bytes())
            .await
            .expect("append the copy");

        sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("sync it back");
        let folder_id = store
            .folders_for_account(acc)
            .unwrap()
            .into_iter()
            .find(|f| f.name == folder)
            .expect("the folder")
            .id;
        let rows = store.drafts_in_folder(folder_id, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].message_id.as_deref(),
            Some(mid.as_str()),
            "the id must survive ENVELOPE + decode_header untouched, or the dedup stops matching"
        );

        // And the payoff, end to end: the draft and its copy are ONE row in the Drafts list.
        assert_eq!(
            rows.len(),
            1,
            "one copy on the server for one draft — a re-save replaces it, never appends"
        );
        let _ = delete_folder(&cfg, &secrets, &folder).await;
    }

    /// New-mail detection (NOTIF-1): the FIRST sync of a folder primes it and announces nothing (or a
    /// new account would notify about its whole inbox); after that, an arriving message is announced,
    /// one already `\Seen` on the server is not, and a re-sync announces nothing again.
    /// Needs Dovecot + `--features dangerous-tls`.
    #[cfg(feature = "dangerous-tls")]
    #[tokio::test]
    #[ignore = "requires local Dovecot with the geleittest user + --features dangerous-tls"]
    async fn live_new_mail_is_detected_and_first_sync_is_silent() {
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
        // Its OWN mailbox, not the shared INBOX: sibling live tests append there, and their arrivals
        // would land in this test's counts (and this test's appends would break theirs). Hermetic.
        let folder = format!("GeleitNotif{}", std::process::id());
        let _ = delete_folder(&cfg, &secrets, &folder).await; // leftovers from a failed run
        create_folder(&cfg, &secrets, &folder)
            .await
            .expect("create");

        let raw = |subject: &str| {
            format!("Subject: {subject}\r\nFrom: Alice <alice@example.com>\r\n\r\nBody.\r\n")
        };
        let store = Store::open_in_memory().unwrap();
        let acc = store.add_account("geleittest@localhost", None).unwrap();

        // 1) FIRST sync of a folder — primes it, announces NOTHING. (Even though the folder is empty:
        //    priming is a recorded fact about "have we ever looked", not a guess from the contents.)
        let first = sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("first sync");
        assert!(!first.primed, "a folder we've never synced isn't primed");
        assert!(
            first.worth_announcing().is_empty(),
            "the first sync must never notify — it would announce the whole folder"
        );

        // 2) A new UNSEEN message arrives → announced, with its sender and subject. Note this also
        //    covers the empty-folder case: the very first message must still be news.
        let subject = "Geleit NOTIF new mail";
        append_message(&cfg, &secrets, &folder, "()", raw(subject).as_bytes())
            .await
            .expect("append new");
        let second = sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("second sync");
        assert!(second.primed, "the folder is primed now");
        let news = second.worth_announcing();
        assert_eq!(news.len(), 1, "arrived: {:?}", second.arrived);
        assert_eq!(news[0].subject, subject);
        assert_eq!(news[0].from, "Alice"); // display name, not the bare address

        // 3) A message already read in another client (\Seen on the server) is NOT news.
        append_message(
            &cfg,
            &secrets,
            &folder,
            "(\\Seen)",
            raw("Geleit NOTIF already read").as_bytes(),
        )
        .await
        .expect("append seen");
        let third = sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("third sync");
        assert_eq!(third.arrived.len(), 1, "it did arrive…");
        assert!(
            third.worth_announcing().is_empty(),
            "…but it was already read elsewhere, so it isn't news"
        );

        // 4) A sync with nothing new announces nothing.
        let fourth = sync_folder_incremental(&cfg, &secrets, &store, acc, &folder, 50)
            .await
            .expect("fourth sync");
        assert!(fourth.arrived.is_empty());
        assert!(fourth.worth_announcing().is_empty());

        delete_folder(&cfg, &secrets, &folder)
            .await
            .expect("clean up");
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
