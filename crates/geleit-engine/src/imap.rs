//! IMAP connectivity — connect to one account over TLS, log in, and list folders (ACC-3,
//! READ-6). Async (`tokio` + `async-imap`), TLS via `rustls`/`ring` (ADR-0006). Credentials come
//! from the platform [`SecretStore`] seam (SEC-2) and are **never logged** (constitution P2).

use std::sync::Arc;

use futures::StreamExt;
use geleit_platform::secret::{SecretError, SecretStore};
use geleit_store::{Store, StoreError};
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

/// Connect to the IMAP server, log in, list folders, and log out. Returns the folder names.
pub async fn list_folders(
    config: &ImapConfig,
    secrets: &dyn SecretStore,
) -> Result<Vec<String>, ImapError> {
    // Fetch the password first: missing → fail before opening any socket.
    // NOTE: `SecretStore::get` is sync; the in-memory double is instant, but when the real
    // OS-keychain backend lands (libsecret/DBus) this should move behind `spawn_blocking` so it
    // doesn't block the async executor (guidelines §5).
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

    let mut session = client
        .login(&config.username, &password)
        .await
        .map_err(|(err, _client)| err)?;

    let mut folders = Vec::new();
    {
        let mut names = session.list(Some(""), Some("*")).await?;
        while let Some(name) = names.next().await {
            folders.push(name?.name().to_string());
        }
    }
    // Best-effort close: we already have the folders, so a transient LOGOUT failure shouldn't
    // turn a successful result into an error.
    let _ = session.logout().await;
    Ok(folders)
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
}
