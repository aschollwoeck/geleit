//! Safe-HTML sanitization for the reading pane.
//!
//! The security model (constitution PRIV-1/PRIV-4) is **layered**, and the sanitizer is deliberately
//! *permissive about presentation* so real mail looks like real mail (S3.6):
//!
//! - **Formatting is kept** — inline `style`, `<style>` blocks, `class`, `<font>`, and presentational
//!   attributes (`bgcolor`, `align`, `width`…) all survive, so colors/fonts/layout render.
//! - **Links are kept** (`http(s)`/`mailto` `href`) — links never auto-load; following one is a
//!   user action.
//! - **Remote images are blocked by default** (PRIV-1): an `<img src>` pointing at the network is
//!   stripped unless the person opts in per message ([`sanitize_html_allowing_remote`]). `cid:`/
//!   `data:` (inline) images are kept.
//! - **Scripts can never run** (PRIV-4): `<script>`, `on*` handlers, and `javascript:`/`vbscript:`
//!   URLs are removed here, AND the message renders inside a `default-src 'none'` CSP
//!   ([`webview_document`]) with no `script-src`, in a sandboxed iframe with no `allow-scripts`.
//!
//! CSS is *not* deeply parsed; its safety rests on the CSP, which is the real network boundary —
//! a CSS `url(http://…)` is blocked by the browser regardless of what the sanitizer let through, and
//! modern WebKit ignores legacy script-in-CSS (`expression()`/`behavior`). A CSS `url()` is invisible
//! to the sanitize-vs-sanitize diff, so [`has_remote_content`] scans for it separately — the CSP
//! *blocks* it either way, but the user should still be *told* it was there. Pure — unit- and
//! mutation-tested.
//!
//! [`webview_document`] wraps the sanitized body in the CSP'd document served to the OS webview from
//! its own `mail://` origin (M9, ADR-0012). (Its Slint/Blitz-era predecessor `document()`, with two
//! Blitz workarounds, was deleted in S9.7.)

use std::borrow::Cow;
use std::collections::HashSet;

/// Presentational attributes emails rely on, allowed on every tag (none of them carry a remote URL —
/// remote loading is gated separately on `img src` + the CSP).
const PRESENTATION_ATTRS: [&str; 17] = [
    "style",
    "class",
    "align",
    "valign",
    "bgcolor",
    "color",
    "width",
    "height",
    "dir",
    "face",
    "size",
    "border",
    "cellpadding",
    "cellspacing",
    "colspan",
    "rowspan",
    "nowrap",
];

/// Build a sanitizer configured to **keep formatting** (styles, classes, `<style>`, links).
fn formatting_builder() -> ammonia::Builder<'static> {
    let mut b = ammonia::Builder::default();
    b.url_schemes(HashSet::from(["mailto", "cid", "http", "https", "data"]))
        // deny relative URLs (incl. scheme-relative `//host`) — they can't resolve under the null
        // base URI anyway, and it keeps the remote/no-remote diff (the cue) precise.
        .url_relative(ammonia::UrlRelative::Deny)
        .add_tags(["style", "font", "center"]) // keep <style> blocks + legacy formatting tags
        .rm_clean_content_tags(["style"]) // ...and keep the CSS inside <style>, don't drop it
        .add_generic_attributes(PRESENTATION_ATTRS);
    b
}

/// Sanitize raw email HTML for safe display with formatting intact, **blocking remote content** by
/// default (PRIV-1): remote `<img src>` is stripped; scripts/handlers are removed (PRIV-4).
pub fn sanitize_html(raw: &str) -> String {
    clean(raw, true)
}

/// Sanitize but **keep** remote images — used only when the person explicitly opts in to load remote
/// content for one message (PRIV-2). Scripts/handlers are still removed (PRIV-4 is never relaxed).
pub fn sanitize_html_allowing_remote(raw: &str) -> String {
    clean(raw, false)
}

fn clean(raw: &str, block_remote_images: bool) -> String {
    let mut b = formatting_builder();
    b.attribute_filter(move |element, attribute, value| {
        // Only real link schemes survive in `href`. A `data:`/`javascript:`/relative href would let
        // a click navigate the webview *away* from our CSP'd document (a `data:text/html` link =
        // un-sandboxed phishing page + opt-in bypass); the app also routes link clicks to the
        // system browser, so in-pane navigation never happens for these.
        if attribute == "href" && !is_safe_link(value) {
            return None;
        }
        // Remote images are blocked by default (PRIV-1) unless the person opted in for this message.
        if block_remote_images && element == "img" && attribute == "src" && is_remote_url(value) {
            return None;
        }
        Some(Cow::Borrowed(value))
    });
    b.clean(raw).to_string()
}

/// Whether an `<img src>` would fetch from the network if rendered. `cid:` and `data:` are inline
/// (embedded in the message — no network); everything else reaching here (http/https; relative is
/// already denied by the builder) is remote.
fn is_remote_url(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    !(v.starts_with("cid:") || v.starts_with("data:"))
}

/// Whether a `href` is a real, safe link to keep. Only `http(s)`/`mailto` — never `data:`,
/// `javascript:`, or relative refs.
fn is_safe_link(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    v.starts_with("http:") || v.starts_with("https:") || v.starts_with("mailto:")
}

/// Does CSS reference something remote — `background:url(http://tracker/…)` or the string form
/// `@import "http://tracker/x.css"`?
///
/// The sanitizer deliberately leaves `style`/`<style>` alone (the CSP, not the sanitizer, is what
/// stops CSS loading remote resources), so a CSS-based tracker is **invisible** to a
/// sanitize-vs-sanitize comparison. It is still *blocked* by the CSP — but without this the user
/// would never be *told*, and "Load images" would never appear for a tracker that hides in CSS.
///
/// A **best-effort cue, not the security boundary** (the CSP is). It catches the two common
/// remote-CSS forms: `url(…)` — which covers `background`, `list-style-image`, `filter`, `cursor`,
/// `image-set`, … — and the string form of `@import` (its `url(…)` form is caught by the first
/// scan). Anything not clearly local (`data:`, `cid:`, a fragment) counts as remote, so we err
/// toward showing the cue.
fn css_references_remote(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();

    // `url(...)` in any property, in any quoting style.
    let mut rest = lower.as_str();
    while let Some(i) = rest.find("url(") {
        rest = &rest[i + 4..];
        let v = rest.trim_start().trim_start_matches(['"', '\'']);
        if !(v.starts_with("data:") || v.starts_with("cid:") || v.starts_with('#')) {
            return true;
        }
    }

    // `@import "http://…"` / `@import '…'` — the string form has no `url(`, so the scan above misses
    // it. This was the gap the S9.2 review flagged: a CSS tracker with no `url()`.
    let mut rest = lower.as_str();
    while let Some(i) = rest.find("@import") {
        rest = &rest[i + 7..];
        let v = rest.trim_start().trim_start_matches(['"', '\'']);
        if v.starts_with("http:") || v.starts_with("https:") || v.starts_with("//") {
            return true;
        }
    }
    false
}

/// Does this message carry remote content that we blocked (PRIV-3)?
///
/// Two signals, because one alone misses half the trackers:
/// 1. the two sanitizations **differ** — something remote was stripped from an attribute (`<img
///    src="https://…">`, a remote `<link>`); and
/// 2. the surviving CSS still **references** something remote (see [`css_references_remote`]).
///
/// A plain `<a href="https://…">` is *not* remote content: it fetches nothing until clicked.
#[must_use]
pub fn has_remote_content(raw: &str) -> bool {
    // Cheap reject: a message with no scheme and no `url(` at all cannot reference anything remote,
    // so skip the two sanitize passes entirely — the overwhelmingly common case for simple mail.
    // (`//` catches scheme-relative `//host`.) This keeps message-open instant on plain HTML.
    let lower = raw.to_ascii_lowercase();
    if !lower.contains("://") && !lower.contains("//") && !lower.contains("url(") {
        return false;
    }
    let blocked = sanitize_html(raw);
    blocked != sanitize_html_allowing_remote(raw) || css_references_remote(&blocked)
}

/// Wrap already-sanitized body HTML in a document for the **OS webview** (M9, ADR-0012).
///
/// A strict CSP — `default-src 'none'`, no `script-src` at all, and `img-src` as the only directive
/// ever relaxed (on explicit opt-in, PRIV-2). This is the *only* document wrapper: M9's predecessor,
/// `document()`, carried two Blitz workarounds (`border-collapse:separate!important`, which is
/// *actively wrong* for a real engine, and a font-fallback pass) and was deleted with the Blitz
/// renderer in S9.7.
///
/// This document is served from its **own origin** (`mail://`), never `srcdoc` — a `srcdoc` iframe
/// *inherits* the embedding page's CSP, which would silently strip the message's inline styles.
///
/// `<base target="_blank">` makes a link click surface as a new-window request, which the shell
/// intercepts and hands to the system browser. The page background stays "paper" light in both app
/// themes: mail is authored for a light background, and recolouring it would misrepresent the sender.
#[must_use]
pub fn webview_document(sanitized_body: &str, allow_remote_images: bool) -> String {
    // Opt-in widens images to **https: only** — never cleartext http: (ADR-0012). A tracking beacon
    // over http would be visible to any on-path observer; a reader who opted in still shouldn't be
    // exposed to that. Inline `data:`/`cid:` are always fine.
    let img_src = if allow_remote_images {
        "data: cid: https:"
    } else {
        "data: cid:"
    };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; \
img-src {img_src}; style-src 'unsafe-inline'; font-src data:; form-action 'none'; base-uri 'none'\">\
         <base target=\"_blank\">\
         <style>html{{font-family:system-ui,sans-serif;color:#1f2a2e;background:#fff;\
margin:0;padding:14px 16px;line-height:1.5}}a{{color:#1c7e7b}}\
img{{max-width:100%;height:auto}}</style>\
         </head><body>{sanitized_body}</body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        has_remote_content, sanitize_html, sanitize_html_allowing_remote, webview_document,
    };

    #[test]
    fn css_url_detection_distinguishes_local_from_remote() {
        use super::css_references_remote;
        // remote — in any quoting style
        assert!(css_references_remote("background:url(http://x/y.png)"));
        assert!(css_references_remote("background:url('https://x/y.png')"));
        assert!(css_references_remote(r#"background:url("//cdn/y.png")"#));
        // local / inert — must NOT trip the cue
        assert!(!css_references_remote(
            "background:url(data:image/gif;base64,AA)"
        ));
        assert!(!css_references_remote("background:url(cid:logo)"));
        assert!(!css_references_remote("clip-path:url(#mask)")); // same-document fragment
        assert!(!css_references_remote("color:#fff; padding:4px")); // no url() at all
                                                                    // the SECOND url() is what's remote — a single scan of the first must not stop early
        assert!(css_references_remote(
            "background:url(data:x), url(http://tracker/p.png)"
        ));
        // the @import STRING form (no url()) — the gap the S9.2 review flagged
        assert!(css_references_remote(r#"@import "http://tracker/x.css";"#));
        assert!(css_references_remote("@import '//cdn/x.css';"));
        assert!(!css_references_remote(r#"@import "data:text/css,body{}";"#));
    }

    #[test]
    fn a_css_import_tracker_with_no_url_is_now_surfaced() {
        // Reaches has_remote_content through a full message (this is the review's exact scenario).
        assert!(has_remote_content(
            r#"<style>@import "http://tracker.example/beacon.css";</style><p>hi</p>"#
        ));
    }

    #[test]
    fn plain_mail_short_circuits_before_sanitizing() {
        // No scheme, no url() → the cheap reject returns false without the two sanitize passes.
        assert!(!has_remote_content(
            "<p>Just some <b>words</b> and a fragment <a href=\"#top\">link</a>.</p>"
        ));
    }

    #[test]
    fn remote_content_is_detected_only_when_something_was_actually_stripped() {
        assert!(has_remote_content(
            r#"<img src="https://tracker.example/pixel.gif">"#
        ));
        assert!(has_remote_content(
            r#"<p style="background:url('http://x.example/t.png')">hi</p>"#
        ));
        // inline + cid images are not remote — no cue should appear for them
        assert!(!has_remote_content(
            r#"<img src="data:image/gif;base64,R0lGOD"><img src="cid:logo">"#
        ));
        assert!(!has_remote_content("<p>just words</p>"));
        // a plain http *link* is not remote content — it fetches nothing until clicked
        assert!(!has_remote_content(
            r#"<a href="https://example.com">x</a>"#
        ));
    }

    #[test]
    fn the_webview_document_blocks_remote_images_by_default_and_never_allows_script() {
        let doc = webview_document("<p>hi</p>", false);
        assert!(doc.contains("default-src 'none'"));
        assert!(doc.contains("img-src data: cid:;"));
        assert!(!doc.contains("https:"), "remote images must be blocked");
        assert!(
            !doc.contains("script-src"),
            "no script-src at all — scripts are never permitted, even on opt-in"
        );
        assert!(doc.contains("form-action 'none'"));
    }

    #[test]
    fn opting_in_widens_img_src_to_https_only_never_cleartext() {
        let doc = webview_document("<p>hi</p>", true);
        assert!(doc.contains("img-src data: cid: https:;"));
        // cleartext http: images are NOT loaded even on opt-in — a beacon over http is on-path visible
        assert!(!doc.contains("http:;"));
        assert!(!doc.contains("script-src"));
        assert!(doc.contains("default-src 'none'"));
    }

    /// The Blitz workarounds must NEVER reach a real browser engine: `border-collapse:separate` would
    /// corrupt every email that legitimately collapses its table borders.
    #[test]
    fn the_webview_document_carries_no_blitz_workarounds() {
        let doc = webview_document("<table><tr><td>x</td></tr></table>", false);
        assert!(!doc.contains("border-collapse"));
        assert!(!doc.contains("border-spacing"));
    }

    #[test]
    fn strips_scripts_and_handlers() {
        let out = sanitize_html(
            "<p>hi</p><script>alert('x')</script><a href=\"javascript:steal()\" onclick=\"x()\">l</a>",
        );
        assert!(!out.contains("<script"), "out: {out}");
        assert!(!out.contains("alert"), "out: {out}");
        assert!(!out.contains("javascript:"), "out: {out}");
        assert!(!out.contains("onclick"), "out: {out}");
        assert!(!out.contains("steal"), "out: {out}");
        assert!(out.contains("hi"));
    }

    #[test]
    fn blocks_data_and_unsafe_schemes_in_links() {
        let out = sanitize_html(
            "<a href=\"data:text/html,<b>x</b>\">a</a>\
             <a href=\"https://ok.test\">b</a><a href=\"mailto:x@y.z\">c</a>",
        );
        assert!(!out.contains("data:text/html"), "data: link blocked: {out}");
        assert!(out.contains("https://ok.test"), "http link kept: {out}");
        assert!(out.contains("mailto:x@y.z"), "mailto link kept: {out}");
    }

    #[test]
    fn keeps_formatting_styles_and_classes() {
        // the whole point of S3.6: styling survives so mail looks like mail
        let out = sanitize_html(
            "<div style=\"color:#234;background:#eef\"><h1 style=\"color:teal\">Hi</h1>\
             <style>.x{color:red}</style><span class=\"x\">s</span>\
             <font color=\"blue\">f</font>\
             <table><tr><td bgcolor=\"#ccc\" align=\"center\">c</td></tr></table></div>",
        );
        assert!(
            out.contains("style=\"color:#234"),
            "inline style kept: {out}"
        );
        assert!(out.contains("<style"), "<style> kept: {out}");
        assert!(out.contains("class=\"x\""), "class kept: {out}");
        assert!(out.contains("<font"), "font kept: {out}");
        assert!(out.contains("bgcolor"), "bgcolor kept: {out}");
        assert!(out.contains("align"), "align kept: {out}");
    }

    #[test]
    fn keeps_links_but_blocks_remote_images() {
        let out = sanitize_html(
            "<a href=\"https://example.com\">link</a>\
             <img src=\"https://tracker.example/p.gif\">\
             <img src=\"//tracker.example/q.gif\">\
             <img src=\"cid:logo123\">\
             <img src=\"data:image/png;base64,AAAA\">",
        );
        // links are kept (they don't auto-load)
        assert!(out.contains("https://example.com"), "link kept: {out}");
        // remote images (absolute + scheme-relative) are stripped
        assert!(
            !out.contains("tracker.example"),
            "remote image blocked: {out}"
        );
        // inline images (cid: AND data:) are kept — they don't hit the network
        assert!(out.contains("cid:logo123"), "cid image kept: {out}");
        assert!(out.contains("data:image/png"), "data image kept: {out}");
    }

    #[test]
    fn allowing_remote_keeps_images_but_still_strips_scripts() {
        let out = sanitize_html_allowing_remote(
            "<img src=\"https://cdn.example/x.png\"><script>steal()</script>",
        );
        assert!(
            out.contains("https://cdn.example/x.png"),
            "remote image kept: {out}"
        );
        assert!(!out.contains("<script"), "script still stripped: {out}");
        assert!(!out.contains("steal"), "out: {out}");
    }

    #[test]
    fn remote_cue_detection_compares_bodies() {
        // the app shows the "remote blocked" cue iff the two sanitizers' bodies differ.
        let plain = "<h1 style=\"color:teal\">Hi</h1><a href=\"https://x.test\">l</a>";
        assert_eq!(
            sanitize_html(plain),
            sanitize_html_allowing_remote(plain),
            "no remote image must not trip the cue (links/styles are not remote)"
        );
        let remote = "<img src=\"https://cdn.example/x.png\"><p>hi</p>";
        assert_ne!(
            sanitize_html(remote),
            sanitize_html_allowing_remote(remote),
            "a remote image must trip the cue"
        );
    }

    #[test]
    fn drops_dangerous_tags() {
        let out = sanitize_html(
            "<iframe src=\"http://x/\"></iframe><base href=\"http://x/\">\
             <meta http-equiv=\"refresh\" content=\"0;url=http://x/\"><object data=\"http://x/\"></object>",
        );
        assert!(!out.contains("<iframe"), "out: {out}");
        assert!(!out.contains("<base"), "out: {out}");
        assert!(!out.contains("<meta"), "out: {out}");
        assert!(!out.contains("<object"), "out: {out}");
    }
}
