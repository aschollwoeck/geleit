//! Optional HTTP Basic auth for the web host (ADR-0014), for reaching GeleitMail across a LAN.
//!
//! **Off by default.** With no password configured the server is localhost-only and this layer waves
//! every request through — today's behaviour, unchanged. When `GELEIT_PASSWORD` is set (which the
//! startup fail-safe *requires* before binding anything but loopback), every request must carry a
//! matching Basic credential, and the browser's native login prompt collects it once.
//!
//! Why Basic and not a login page: once the browser has the credential for the origin it attaches it
//! to *every* request automatically — the `/invoke` fetches, the `/events` SSE stream, the `/download`
//! links, the `/mail` iframe — so the whole WASM UI works behind auth with **zero** frontend changes.
//!
//! Basic sends the credential base64-encoded (i.e. cleartext), so it is only safe over HTTPS. This
//! server speaks plain HTTP and is meant to sit behind a TLS-terminating reverse proxy on the LAN
//! (topology A — see the README); the fail-safe + this note are the guard rails, not in-app TLS.
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use std::sync::Arc;

/// The configured password, or `None` for localhost/no-auth mode. Cheap to clone into the layer.
pub type AuthState = Option<Arc<str>>;

/// The auth gate. `None` state → pass through; `Some(pw)` → require a Basic credential whose password
/// half equals `pw` (the username is ignored — one shared password, single-user).
pub async fn require_auth(State(password): State<AuthState>, req: Request, next: Next) -> Response {
    let Some(password) = password else {
        return next.run(req).await; // no-auth (localhost) mode
    };
    if request_password(&req)
        .is_some_and(|got| constant_time_eq(got.as_bytes(), password.as_bytes()))
    {
        next.run(req).await
    } else {
        unauthorized()
    }
}

/// Pull the password half out of an `Authorization: Basic base64(user:pass)` header, if present and
/// well-formed. Returns `None` (→ 401) for a missing/malformed header rather than erroring.
fn request_password(req: &Request) -> Option<String> {
    let header = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    // The auth-scheme token is case-insensitive (RFC 7617), so accept `Basic`/`basic`/`BASIC`/…
    let (scheme, b64) = header.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .ok()?;
    let creds = String::from_utf8(decoded).ok()?;
    // "user:pass" — split on the FIRST colon; the password may itself contain colons.
    let (_user, pass) = creds.split_once(':')?;
    Some(pass.to_owned())
}

/// 401 with the `WWW-Authenticate` challenge that makes the browser show its login prompt.
fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(
            header::WWW_AUTHENTICATE,
            "Basic realm=\"GeleitMail\", charset=\"UTF-8\"",
        )],
        "Authentication required.",
    )
        .into_response()
}

/// Length-checked, constant-time byte comparison, so a wrong password can't be recovered by timing the
/// response. (The length is allowed to leak — comparing only equal-length inputs in constant time is
/// the standard, sufficient guarantee here.)
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic(user: &str, pass: &str) -> String {
        let b64 = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
        format!("Basic {b64}")
    }

    fn req_with_auth(value: Option<&str>) -> Request {
        let mut b = Request::builder().uri("/");
        if let Some(v) = value {
            b = b.header(header::AUTHORIZATION, v);
        }
        b.body(axum::body::Body::empty()).unwrap()
    }

    #[test]
    fn extracts_the_password_half_ignoring_the_username() {
        let r = req_with_auth(Some(&basic("anyone", "hunter2")));
        assert_eq!(request_password(&r).as_deref(), Some("hunter2"));
    }

    #[test]
    fn a_password_may_contain_colons() {
        let r = req_with_auth(Some(&basic("u", "a:b:c")));
        assert_eq!(request_password(&r).as_deref(), Some("a:b:c"));
    }

    #[test]
    fn missing_or_malformed_headers_yield_none_not_a_panic() {
        assert_eq!(request_password(&req_with_auth(None)), None);
        assert_eq!(request_password(&req_with_auth(Some("Bearer xyz"))), None);
        assert_eq!(
            request_password(&req_with_auth(Some("Basic !!!not-base64"))),
            None
        );
        assert_eq!(request_password(&req_with_auth(Some("Basic"))), None); // no space/credentials
    }

    #[test]
    fn the_scheme_token_is_case_insensitive() {
        let b64 = base64::engine::general_purpose::STANDARD.encode("u:pw");
        for scheme in ["Basic", "basic", "BASIC", "BaSiC"] {
            let r = req_with_auth(Some(&format!("{scheme} {b64}")));
            assert_eq!(
                request_password(&r).as_deref(),
                Some("pw"),
                "scheme {scheme}"
            );
        }
    }

    #[test]
    fn constant_time_eq_matches_only_identical_bytes() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreo"));
        assert!(!constant_time_eq(b"secret", b"secre")); // length differs
        assert!(!constant_time_eq(b"", b"x"));
    }
}
