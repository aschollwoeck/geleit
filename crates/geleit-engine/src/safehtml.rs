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

/// Wrap already-sanitized body HTML in a minimal document carrying a strict Content-Security-Policy
/// (defense-in-depth, S3.2). `default-src 'none'` covers all *fetch* directives (script/connect/
/// img/font/frame/object…) so nothing loads or executes even if sanitization missed something;
/// `form-action`/`base-uri` are added explicitly because they don't fall back to `default-src`.
/// `style-src 'unsafe-inline'` is only for this trusted wrapper's `<style>` — body `<style>`/`style=`
/// are stripped by the sanitizer. The CSP `<meta>` lives in this trusted wrapper (the sanitized body
/// can't contain `<meta>`), so it governs.
pub fn document(sanitized_body: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; \
img-src data: cid:; style-src 'unsafe-inline'; font-src data:; form-action 'none'; base-uri 'none'\">\
         <style>html{{font-family:system-ui,sans-serif;color:#1f2a2e;background:#fbfaf7;\
margin:0;padding:12px;line-height:1.5}}a{{color:#1c7e7b}}img{{max-width:100%;height:auto}}</style>\
         </head><body>{sanitized_body}</body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::{document, sanitize_html};

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
    fn document_wraps_with_strict_csp() {
        let doc = document("<p>body here</p>");
        assert!(doc.contains("Content-Security-Policy"), "doc: {doc}");
        assert!(doc.contains("default-src 'none'"), "doc: {doc}");
        assert!(doc.contains("form-action 'none'"), "doc: {doc}");
        assert!(doc.contains("base-uri 'none'"), "doc: {doc}");
        assert!(
            !doc.contains("script-src"),
            "no script-src allowance: {doc}"
        );
        assert!(doc.contains("<p>body here</p>"), "body included: {doc}");
        assert!(doc.starts_with("<!doctype html>"));
    }

    #[test]
    fn sandbox_escape_vectors_are_neutralised() {
        let out = sanitize_html(
            "<a href=\"javascript:steal()\">x</a>\
             <svg onload=\"steal()\"><circle/></svg>\
             <button formaction=\"http://evil/\">y</button>\
             <div style=\"width:expression(steal())\">z</div>\
             <a href=\"vbscript:bad\">w</a>",
        );
        assert!(!out.contains("javascript:"), "out: {out}");
        assert!(!out.contains("vbscript:"), "out: {out}");
        assert!(!out.contains("onload"), "out: {out}");
        assert!(!out.contains("formaction"), "out: {out}");
        assert!(!out.contains("expression"), "out: {out}");
        assert!(!out.contains("steal"), "out: {out}");
    }

    #[test]
    fn obfuscated_script_schemes_and_svg_links_are_neutralised() {
        // case-variant + entity-encoded scheme, and SVG xlink:href — the allowlist must catch these
        let out = sanitize_html(
            "<a href=\"JaVaScRiPt:bad()\">a</a>\
             <a href=\"java&#115;cript:bad()\">b</a>\
             <svg><a xlink:href=\"javascript:bad()\">c</a></svg>",
        );
        assert!(!out.to_lowercase().contains("javascript:"), "out: {out}");
        assert!(!out.contains("bad()"), "out: {out}");
        assert!(!out.contains("<svg"), "out: {out}");
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
