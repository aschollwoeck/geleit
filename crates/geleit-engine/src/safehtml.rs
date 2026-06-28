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
//!   URLs are removed here, AND the host renders inside a `default-src 'none'` CSP ([`document`])
//!   with JavaScript disabled.
//!
//! CSS is *not* deeply parsed; its safety rests on the CSP, which is the real network boundary —
//! a CSS `url(http://…)` is blocked by the browser regardless of what the sanitizer let through, and
//! modern WebKit ignores legacy script-in-CSS (`expression()`/`behavior`). The webview is also
//! clipped to the reading pane, so CSS can't escape it. Pure — unit- and mutation-tested.

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

/// Wrap already-sanitized body HTML in a minimal document carrying a strict Content-Security-Policy
/// (defense-in-depth, S3.2). `default-src 'none'` covers all *fetch* directives (script/connect/
/// img/font/frame/object…) so nothing loads or executes even if sanitization missed something;
/// `form-action`/`base-uri` are added explicitly because they don't fall back to `default-src`.
/// `style-src 'unsafe-inline'` permits the email's own inline styles + `<style>` (the CSP, not the
/// sanitizer, is what stops CSS from loading remote resources). Only `img-src` is ever relaxed, and
/// only on explicit opt-in (PRIV-2); scripts stay blocked regardless (no `script-src`).
pub fn document(sanitized_body: &str, allow_remote_images: bool) -> String {
    let img_src = if allow_remote_images {
        "data: cid: https: http:"
    } else {
        "data: cid:"
    };
    let body = add_font_fallbacks(sanitized_body);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; \
img-src {img_src}; style-src 'unsafe-inline'; font-src data:; form-action 'none'; base-uri 'none'\">\
         <style>html{{font-family:system-ui,sans-serif;color:#1f2a2e;background:#fbfaf7;\
margin:0;padding:12px;line-height:1.5}}a{{color:#1c7e7b}}img{{max-width:100%;height:auto}}\
table{{border-collapse:separate!important;border-spacing:0!important}}</style>\
         </head><body>{body}</body></html>"
    )
}

/// The CSS generic font families. A `font-family` value naming one of these has a guaranteed
/// fallback that's actually installed.
const GENERIC_FAMILIES: [&str; 11] = [
    "sans-serif",
    "serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-sans-serif",
    "ui-serif",
    "ui-monospace",
    "math",
    "emoji",
];

/// Whether a `font-family` value already lists a generic family (so a missing named font falls
/// through to it). Token-based so e.g. `"PT Serif"` (a named font) isn't mistaken for the generic.
fn font_value_has_generic(value: &str) -> bool {
    value.split(',').any(|t| {
        let t = t.trim().trim_matches(['"', '\'']).to_ascii_lowercase();
        GENERIC_FAMILIES.contains(&t.as_str())
    })
}

/// Append a `, sans-serif` fallback to every `font-family` value (inline `style=` or in `<style>`)
/// that names no generic family. Blitz/parley drops **digit** glyphs for a named-but-uninstalled
/// font (e.g. `font-family:Helvetica` on a box → "15.000" renders as "."); falling through to an
/// installed generic restores them. A value that already has a generic is left untouched.
pub fn add_font_fallbacks(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len() + 64);
    let mut i = 0;
    while let Some(rel) = lower[i..].find("font-family") {
        let prop_start = i + rel;
        let mut j = prop_start + "font-family".len();
        while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b':' {
            // "font-family" not used as a CSS property here — copy past it and continue
            out.push_str(&html[i..prop_start + "font-family".len()]);
            i = prop_start + "font-family".len();
            continue;
        }
        let val_start = j + 1;
        // value ends at the CSS terminators or the `"` that closes an inline `style="…"` attribute
        // (sanitizer output uses double-quoted attributes, so single quotes only wrap font names).
        let val_end = html[val_start..]
            .find([';', '}', '"', '<'])
            .map_or(html.len(), |p| val_start + p);
        let value = &html[val_start..val_end];
        out.push_str(&html[i..val_end]); // copy through the value verbatim
        if !value.trim().is_empty() && !font_value_has_generic(value) {
            out.push_str(", sans-serif");
        }
        i = val_end;
    }
    out.push_str(&html[i..]);
    out
}

#[cfg(test)]
mod tests {
    use super::{add_font_fallbacks, document, sanitize_html, sanitize_html_allowing_remote};

    #[test]
    fn font_fallback_added_only_when_missing() {
        // bare named font → fallback appended (so digits render)
        assert_eq!(
            add_font_fallbacks(r#"<p style="font-family:Helvetica">x</p>"#),
            r#"<p style="font-family:Helvetica, sans-serif">x</p>"#
        );
        // already has a generic → unchanged
        let ok = r#"<p style="font-family:Arial, sans-serif">x</p>"#;
        assert_eq!(add_font_fallbacks(ok), ok);
        // a <style> block value is handled too
        assert_eq!(
            add_font_fallbacks("<style>.a{font-family:Roboto;color:red}</style>"),
            "<style>.a{font-family:Roboto, sans-serif;color:red}</style>"
        );
        // "PT Serif" is a named font, not the `serif` generic → fallback still added
        assert!(
            add_font_fallbacks(r#"<i style="font-family:'PT Serif'">x</i>"#).contains("sans-serif")
        );
        // no font-family at all → untouched
        let plain = "<p>just text 12345</p>";
        assert_eq!(add_font_fallbacks(plain), plain);
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

    #[test]
    fn document_blocks_remote_images_by_default() {
        let doc = document("<p>x</p>", false);
        assert!(doc.contains("default-src 'none'"), "doc: {doc}");
        assert!(doc.contains("form-action 'none'"), "doc: {doc}");
        assert!(doc.contains("base-uri 'none'"), "doc: {doc}");
        assert!(!doc.contains("script-src"), "no script-src: {doc}");
        assert!(
            doc.contains("img-src data: cid:;"),
            "remote img blocked: {doc}"
        );
        assert!(doc.contains("<p>x</p>"));
        assert!(doc.starts_with("<!doctype html>"));
    }

    #[test]
    fn document_opt_in_relaxes_only_images_never_scripts() {
        let doc = document("<p>x</p>", true);
        assert!(
            doc.contains("img-src data: cid: https: http:"),
            "doc: {doc}"
        );
        assert!(
            doc.contains("default-src 'none'"),
            "scripts still blocked: {doc}"
        );
        assert!(!doc.contains("script-src"), "doc: {doc}");
    }
}
