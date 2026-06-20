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
        subject: env.and_then(|e| crate::envelope::decode_header(e.subject.as_deref())),
        from_name,
        from_addr,
        date: f.internal_date().map(|d| d.timestamp()),
        seen: f
            .flags()
            .any(|fl| matches!(fl, async_imap::types::Flag::Seen)),
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

/// Upsert the given folder names into the store under `account_id` (idempotent). Pure — no network.
pub fn persist_folders(
    store: &Store,
    account_id: i64,
    folders: &[String],
) -> Result<(), StoreError> {
    for name in folders {
        store.upsert_folder(account_id, name)?;
    }
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

/// Dev-only certificate verifier, compiled only with the `dangerous-tls` feature.
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
}
