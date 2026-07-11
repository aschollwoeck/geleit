//! The `mail://` origin — where a message's HTML is served (S9.2, ADR-0012).
//!
//! **Why a custom scheme and not `srcdoc`:** a `srcdoc` iframe *inherits the embedding document's
//! CSP*. The app's CSP is deliberately strict (`style-src 'self'`, no `'unsafe-inline'`), so mail
//! delivered via `srcdoc` would have every message's inline styles silently stripped — all mail
//! would render unstyled, with no visible cause. A custom scheme gives the message its **own
//! origin**, so it carries exactly the CSP [`geleit_engine::safehtml::webview_document`] hands it
//! and inherits nothing from the shell.
//!
//! The message body therefore never enters the app's document, not even as a string: the frontend
//! only ever points an `<iframe>` at `mail://localhost/<id>`.
//!
//! URL shape: `mail://localhost/<message-id>` (`?images=1` opts that one message in to remote
//! images, PRIV-2). Windows serves custom schemes as `http://mail.localhost/...`; both are handled.
use crate::ipc::{message_html, AppState};
use tauri::http::{header::CONTENT_TYPE, Request, Response};
use tauri::{Manager, UriSchemeContext, Wry};

/// A calm, inert page for the cases where there is nothing to show. Same locked-down CSP, so even
/// the error path can't fetch or run anything.
fn placeholder(text: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; \
style-src 'unsafe-inline'\">\
         <style>html{{font-family:system-ui,sans-serif;color:#5e7177;background:#fff;\
margin:0;padding:28px;text-align:center}}</style></head><body>{text}</body></html>"
    )
}

/// The policy for this response. Kept **identical** to the one `webview_document` writes into the
/// page: browsers enforce every policy they're given, so a header and a meta that disagree would
/// silently intersect, and reasoning about "what is actually allowed" would become guesswork.
/// Sending it as a header too means the policy holds even if the markup weren't parsed as expected.
fn csp(allow_remote_images: bool) -> String {
    // https: only on opt-in — never cleartext http: (ADR-0012; matches webview_document exactly,
    // which a test enforces).
    let img_src = if allow_remote_images {
        "data: cid: https:"
    } else {
        "data: cid:"
    };
    format!(
        "default-src 'none'; img-src {img_src}; style-src 'unsafe-inline'; font-src data:; \
         form-action 'none'; base-uri 'none'"
    )
}

fn html_response(body: String, allow_remote_images: bool) -> Response<Vec<u8>> {
    Response::builder()
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .header("Content-Security-Policy", csp(allow_remote_images))
        .body(body.into_bytes())
        .expect("static response")
}

/// Serve `mail://localhost/<id>[?images=1]`.
pub fn handle(ctx: UriSchemeContext<'_, Wry>, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let uri = request.uri();
    let id: Option<i64> = uri
        .path()
        .trim_matches('/')
        .split('/')
        .next_back()
        .and_then(|s| s.parse().ok());
    let allow_remote = uri.query().is_some_and(|q| {
        q.split('&')
            .any(|kv| matches!(kv, "images=1" | "images=true"))
    });

    let Some(id) = id else {
        return html_response(placeholder("No message selected."), false);
    };
    let state = ctx.app_handle().state::<AppState>();
    match message_html(&state, id, allow_remote) {
        Some(doc) => html_response(doc, allow_remote),
        None => html_response(placeholder("This message has no formatted content."), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Even the "nothing here" page must be inert — it is still rendered inside the mail origin.
    #[test]
    fn the_placeholder_cannot_fetch_or_run_anything() {
        let p = placeholder("No message selected.");
        assert!(p.contains("default-src 'none'"));
        assert!(!p.contains("script"));
    }

    #[test]
    fn the_response_carries_a_csp_header_as_well_as_the_meta() {
        let r = html_response("<p>x</p>".to_owned(), false);
        let header = r.headers().get("Content-Security-Policy").unwrap();
        let header = header.to_str().unwrap();
        assert!(header.contains("default-src 'none'"));
        assert!(
            !header.contains("script-src"),
            "scripts are never permitted in the mail origin"
        );
        assert!(header.contains("form-action 'none'"));
    }

    /// The header and the in-page meta must say the SAME thing. If they diverged, browsers would
    /// enforce both and quietly intersect them — and "what is actually allowed" becomes guesswork.
    #[test]
    fn the_header_csp_matches_the_documents_own_csp() {
        for allow in [false, true] {
            let doc = geleit_engine::safehtml::webview_document("<p>x</p>", allow);
            assert!(
                doc.contains(&csp(allow)),
                "header CSP diverged from the document's, allow_remote={allow}"
            );
        }
    }

    #[test]
    fn remote_images_are_blocked_unless_opted_in_and_only_over_https() {
        assert!(!csp(false).contains("https:"));
        assert!(csp(true).contains("img-src data: cid: https:"));
        // never cleartext http: — even on opt-in (ADR-0012)
        assert!(!csp(true).contains("http:;"));
        // ...and opting in NEVER unlocks anything but images
        assert!(!csp(true).contains("script-src"));
        assert!(csp(true).contains("default-src 'none'"));
    }
}
