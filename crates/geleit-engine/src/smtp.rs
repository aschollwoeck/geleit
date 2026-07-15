//! SMTP send over rustls (ADR-0009). **Transport only** — message bytes are built elsewhere (S4.2)
//! and handed here via [`send`]. Async (constitution P1); credentials and addresses are never logged
//! (P2). Errors are calm, PII-free strings.

use std::sync::Once;

use lettre::address::Envelope;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::AsyncSmtpTransport;
use lettre::{Address, AsyncTransport, Tokio1Executor};

/// Transport security for the SMTP connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmtpSecurity {
    /// Implicit TLS from the first byte (typically port 465).
    Implicit,
    /// Upgrade a plaintext connection with STARTTLS (typically port 587).
    StartTls,
    /// No TLS at all — **localhost / testing only**, never a real provider.
    Plaintext,
}

/// How to reach the SMTP server.
#[derive(Clone)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub security: SmtpSecurity,
    /// Accept an invalid/self-signed server certificate — dev only, needs the `dangerous-tls`
    /// feature; ignored otherwise. Never for real providers.
    pub allow_invalid_certs: bool,
}

// Manual Debug that redacts `username` (an email address = PII, P2) so an accidental `{:?}` can't
// leak it. The password is never stored here — it's passed separately to `send`.
impl std::fmt::Debug for SmtpSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpSettings")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &"<redacted>")
            .field("security", &self.security)
            .field("allow_invalid_certs", &self.allow_invalid_certs)
            .finish()
    }
}

/// Build an SMTP envelope (reverse-path sender + recipients) from string addresses. Inputs must be
/// **bare addr-specs** (`user@domain`), not display-name form (`Bob <bob@x>`) — display names belong
/// in the message headers built by the caller (S4.2). At least one recipient is required.
pub fn envelope(from: &str, to: &[String]) -> Result<Envelope, String> {
    let from_addr: Address = from
        .trim()
        .parse()
        .map_err(|_| "The sender address isn't valid.".to_owned())?;
    let mut recipients = Vec::with_capacity(to.len());
    for addr in to {
        recipients.push(
            addr.trim()
                .parse::<Address>()
                .map_err(|_| "A recipient address isn't valid.".to_owned())?,
        );
    }
    if recipients.is_empty() {
        return Err("Add at least one recipient.".to_owned());
    }
    Envelope::new(Some(from_addr), recipients)
        .map_err(|_| "Couldn't build the message envelope.".to_owned())
}

/// Send pre-built RFC 5322 message bytes to the SMTP server. Returns a calm, PII-free error if the
/// server is unreachable or rejects the message.
/// Why a send didn't go out — and, crucially, whether trying again could help (SEND-10).
///
/// `permanent` means the server *answered* and **rejected** the message (a 5xx: a bad address, a
/// policy block); retrying is pointless, so the outbox must not queue it — the user has to fix or drop
/// it. Everything else — couldn't connect, TLS failed, a transient 4xx — is retryable, which is the
/// ordinary "you're offline" case the outbox exists for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendError {
    pub message: String,
    pub permanent: bool,
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Send one message. On failure the error says whether retrying could help — see [`SendError`].
///
/// # Errors
/// [`SendError`] describing what went wrong and whether it's worth retrying.
pub async fn send(
    settings: &SmtpSettings,
    password: &str,
    envelope: &Envelope,
    message: &[u8],
) -> Result<(), SendError> {
    let transport = build_transport(settings, password).map_err(|message| SendError {
        message,
        // A config/build problem is on us, not a rejection — treat it as retryable rather than
        // discarding the user's mail. (In practice this only fires on a malformed setting.)
        permanent: false,
    })?;
    transport
        .send_raw(envelope, message)
        .await
        .map(|_| ())
        .map_err(|e| {
            // `lettre` tells us a 5xx (`is_permanent`) apart from a can't-reach/transient failure.
            let permanent = e.is_permanent();
            SendError {
                message: if permanent {
                    "The server rejected the message.".to_owned()
                } else {
                    "Couldn't reach the outgoing mail server.".to_owned()
                },
                permanent,
            }
        })
}

/// Ensure a process-wide rustls crypto provider (ring) is installed, so lettre's TLS path has one.
/// Idempotent; harmless alongside the IMAP stack, which configures rustls explicitly.
fn ensure_crypto_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn build_transport(
    settings: &SmtpSettings,
    password: &str,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
    ensure_crypto_provider();
    let tls_err = || "Couldn't set up a secure SMTP connection.".to_owned();
    let mut builder = match settings.security {
        SmtpSecurity::Implicit => {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.host).map_err(|_| tls_err())?
        }
        SmtpSecurity::StartTls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.host)
                .map_err(|_| tls_err())?
        }
        SmtpSecurity::Plaintext => {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&settings.host)
        }
    };
    builder = builder.port(settings.port).credentials(Credentials::new(
        settings.username.clone(),
        password.to_owned(),
    ));
    #[cfg(feature = "dangerous-tls")]
    if settings.allow_invalid_certs && settings.security != SmtpSecurity::Plaintext {
        builder = with_dangerous_tls(builder, settings)?;
    }
    Ok(builder.build())
}

/// Dev-only: accept a self-signed certificate (used for a local test server). Off unless the
/// `dangerous-tls` feature is built; never reached for real providers.
#[cfg(feature = "dangerous-tls")]
fn with_dangerous_tls(
    builder: lettre::transport::smtp::AsyncSmtpTransportBuilder,
    settings: &SmtpSettings,
) -> Result<lettre::transport::smtp::AsyncSmtpTransportBuilder, String> {
    use lettre::transport::smtp::client::{Tls, TlsParameters};
    let params = TlsParameters::builder(settings.host.clone())
        .dangerous_accept_invalid_certs(true)
        .build()
        .map_err(|_| "Couldn't set up TLS for the dev server.".to_owned())?;
    let tls = match settings.security {
        SmtpSecurity::StartTls => Tls::Required(params),
        _ => Tls::Wrapper(params),
    };
    Ok(builder.tls(tls))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_rejects_bad_addresses_and_empty_recipients() {
        assert!(envelope("not an address", &["a@b.com".into()]).is_err());
        assert!(envelope("me@b.com", &["nope".into()]).is_err());
        assert!(envelope("me@b.com", &[]).is_err());
        assert!(envelope("me@b.com", &["a@b.com".into(), "c@d.com".into()]).is_ok());
    }
}
