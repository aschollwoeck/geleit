//! Safe-HTML sanitization for the reading pane (PRIV-1: block remote content; PRIV-4: no scripts).
//! Pure — unit- and mutation-tested. `ammonia` strips `<script>` and `on*` event handlers by
//! default; restricting the allowed URL schemes to `mailto`/`cid` drops every `http(s)` reference
//! (remote images, tracking pixels, remote links), so nothing loads from the network when the
//! result is rendered. The sandboxed webview (the host) renders only this sanitized output.

use std::collections::HashSet;

/// Sanitize raw email HTML into a form safe to render: no scripts, no event handlers, no remote
/// references. Only `mailto:`/`cid:` URLs survive.
pub fn sanitize_html(raw: &str) -> String {
    ammonia::Builder::default()
        .url_schemes(HashSet::from(["mailto", "cid"]))
        // Deny relative URLs too — otherwise scheme-relative refs like `//tracker/p.gif` survive
        // ammonia's default `PassThrough` and could load remotely (PRIV-1). Only mailto:/cid: remain.
        .url_relative(ammonia::UrlRelative::Deny)
        .clean(raw)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::sanitize_html;

    #[test]
    fn strips_scripts() {
        let out =
            sanitize_html("<p>hi</p><script>alert('x');window.ipc.postMessage('ran')</script>");
        assert!(!out.contains("<script"), "out: {out}");
        assert!(!out.contains("alert"), "out: {out}");
        assert!(out.contains("hi"));
    }

    #[test]
    fn strips_remote_references() {
        // remote image (tracking pixel) and remote link href are removed → nothing loads remotely
        let out = sanitize_html(
            "<img src=\"http://tracker.example/p.gif\"><a href=\"https://evil.example\">x</a>",
        );
        assert!(!out.contains("http://"), "out: {out}");
        assert!(!out.contains("https://"), "out: {out}");
        assert!(!out.contains("tracker.example"), "out: {out}");
    }

    #[test]
    fn strips_event_handlers() {
        let out = sanitize_html("<a href=\"mailto:a@b.com\" onclick=\"steal()\">link</a>");
        assert!(!out.contains("onclick"), "out: {out}");
        assert!(!out.contains("steal"), "out: {out}");
        // a safe mailto: link survives
        assert!(out.contains("mailto:a@b.com"), "out: {out}");
    }

    #[test]
    fn strips_scheme_relative_and_data_urls() {
        let out = sanitize_html(
            "<img src=\"//tracker.example/p.gif\"><img src=\"data:image/gif;base64,AAAA\">",
        );
        assert!(!out.contains("tracker.example"), "out: {out}");
        assert!(!out.contains("//tracker"), "out: {out}");
        assert!(!out.contains("data:"), "out: {out}");
    }

    #[test]
    fn drops_dangerous_tags() {
        let out = sanitize_html(
            "<iframe src=\"http://x/\"></iframe>\
             <style>body{background:url(http://x/)}</style>\
             <base href=\"http://x/\"><meta http-equiv=\"refresh\" content=\"0;url=http://x/\">",
        );
        assert!(!out.contains("<iframe"), "out: {out}");
        assert!(!out.contains("<style"), "out: {out}");
        assert!(!out.contains("<base"), "out: {out}");
        assert!(!out.contains("<meta"), "out: {out}");
        assert!(!out.contains("http://x"), "out: {out}");
    }

    #[test]
    fn keeps_safe_formatting_and_text() {
        let out = sanitize_html("<h1>Title</h1><p>Some <b>bold</b> text.</p>");
        assert!(out.contains("<h1"));
        assert!(out.contains("<b"));
        assert!(out.contains("Title"));
        assert!(out.contains("bold"));
        assert!(out.contains("Some"));
    }
}
