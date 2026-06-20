//! OAuth loopback seam — desktop OAuth opens the authorization URL in the user's browser and
//! captures the redirect on a localhost port. The real implementation (a loopback HTTP listener
//! plus per-OS browser launch) lands in M7 (ACC-1 / ACC-2).

/// Error obtaining an OAuth authorization code.
#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    /// The loopback flow failed (listener / browser / transport).
    #[error("oauth loopback error: {0}")]
    Backend(String),
    /// The user denied authorization, or the provider returned an error.
    #[error("oauth authorization denied")]
    Denied,
}

/// Runs the desktop OAuth loopback flow and returns the authorization code.
///
/// `auth_url` is the provider authorization URL; `redirect_port` is the localhost port the
/// redirect URI uses. Implementations must never log the returned code or any tokens
/// (constitution P2).
///
/// Note: this signature is **provisional** (S0.5). The real M7 loopback (RFC 8252) is a
/// security-critical path (P6) that must carry a CSRF `state` and PKCE `code_verifier`; the
/// signature will widen accordingly. See ADR-0004.
pub trait OAuthRedirect: Send + Sync {
    fn obtain_code(&self, auth_url: &str, redirect_port: u16) -> Result<String, OAuthError>;
}

/// Test double that returns a preset code with no network or browser interaction.
pub struct FakeOAuthRedirect {
    /// The code returned by [`OAuthRedirect::obtain_code`].
    pub code: String,
}

impl OAuthRedirect for FakeOAuthRedirect {
    fn obtain_code(&self, _auth_url: &str, _redirect_port: u16) -> Result<String, OAuthError> {
        Ok(self.code.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{FakeOAuthRedirect, OAuthError, OAuthRedirect};

    #[test]
    fn errors_display() {
        assert!(OAuthError::Backend("no listener".to_owned())
            .to_string()
            .contains("no listener"));
        assert_eq!(OAuthError::Denied.to_string(), "oauth authorization denied");
    }

    #[test]
    fn fake_returns_preset_code() {
        let r = FakeOAuthRedirect {
            code: "abc123".to_owned(),
        };
        assert_eq!(
            r.obtain_code("https://provider/authorize", 8080).unwrap(),
            "abc123"
        );
    }
}
