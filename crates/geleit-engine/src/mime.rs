//! Pure MIME-body parsing via `mail-parser`. Kept separate from the network code in `imap.rs` so
//! it stays unit- and mutation-tested. Turns raw RFC822 bytes into the plaintext/HTML body, a
//! short snippet, and an attachment flag.

use mail_parser::MessageParser;

/// The parsed pieces of a message body we store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ParsedBody {
    pub plain: Option<String>,
    pub html: Option<String>,
    pub snippet: Option<String>,
    pub has_attachments: bool,
}

/// Parse a raw RFC822 message into its body parts. Returns an empty [`ParsedBody`] if the message
/// can't be parsed (we never panic on hostile input).
pub(crate) fn parse_body(raw: &[u8]) -> ParsedBody {
    let Some(msg) = MessageParser::default().parse(raw) else {
        return ParsedBody::default();
    };
    let plain = msg.body_text(0).map(|c| c.into_owned());
    // Take the first GENUINE text/html part's contents. `body_html(0)` is unsafe here: it indexes
    // `html_body[0]`, which mail-parser fills with a text part (rendered as escaped HTML) when no
    // real HTML precedes it — so for `text/plain` then `text/html` under multipart/mixed it would
    // return the escaped plaintext, not the real HTML.
    let html = msg
        .html_bodies()
        .find(|p| p.is_text_html())
        .and_then(|p| p.text_contents().map(str::to_owned));
    let snippet = plain.as_deref().map(|t| make_snippet(t, 140));
    ParsedBody {
        plain,
        html,
        snippet,
        has_attachments: msg.attachment_count() > 0,
    }
}

/// A short one-line preview: whitespace collapsed to single spaces, truncated to `max` characters.
pub(crate) fn make_snippet(text: &str, max: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::{make_snippet, parse_body};

    const MULTIPART: &[u8] = b"\
From: Tester <tester@example.com>\r\n\
Subject: body test\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"BOUND\"\r\n\
\r\n\
--BOUND\r\n\
Content-Type: multipart/alternative; boundary=\"ALT\"\r\n\
\r\n\
--ALT\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Hello in plain text.\r\n\
--ALT\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p>Hello in <b>HTML</b>.</p>\r\n\
--ALT--\r\n\
--BOUND\r\n\
Content-Type: text/plain; name=\"note.txt\"\r\n\
Content-Disposition: attachment; filename=\"note.txt\"\r\n\
\r\n\
attached file contents\r\n\
--BOUND--\r\n";

    #[test]
    fn parses_plain_html_and_attachment() {
        let b = parse_body(MULTIPART);
        assert!(b.plain.as_deref().unwrap().contains("Hello in plain text"));
        assert!(b.html.as_deref().unwrap().contains("HTML"));
        assert!(b.has_attachments);
        assert!(b
            .snippet
            .as_deref()
            .unwrap()
            .contains("Hello in plain text"));
    }

    #[test]
    fn picks_real_html_part_not_leading_text() {
        // text/plain BEFORE text/html under multipart/mixed: the real HTML must win, not the
        // leading plaintext rendered as escaped HTML.
        let raw = b"\
Subject: x\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"M\"\r\n\
\r\n\
--M\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
leading plaintext\r\n\
--M\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<b>real html</b>\r\n\
--M--\r\n";
        let b = parse_body(raw);
        let html = b.html.as_deref().expect("html present");
        assert!(html.contains("real html"), "html was: {html:?}");
        assert!(!html.contains("leading plaintext"), "html was: {html:?}");
    }

    #[test]
    fn plain_only_has_no_attachment() {
        let raw = b"Subject: x\r\nContent-Type: text/plain\r\n\r\njust text\r\n";
        let b = parse_body(raw);
        // body_text may keep a trailing newline; compare trimmed.
        assert_eq!(b.plain.as_deref().map(str::trim), Some("just text"));
        assert!(b.html.is_none());
        assert!(!b.has_attachments);
    }

    #[test]
    fn unparseable_is_empty_not_panic() {
        let b = parse_body(&[0xff, 0x00, 0xff]);
        assert!(b.plain.is_none() && b.html.is_none() && !b.has_attachments);
    }

    #[test]
    fn snippet_collapses_whitespace_and_truncates() {
        assert_eq!(make_snippet("  hello \n\t world  ", 100), "hello world");
        assert_eq!(make_snippet("aaaa", 2), "aa");
        // truncation is char-safe (no panic on multi-byte)
        assert_eq!(make_snippet("héllo", 2).chars().count(), 2);
    }
}
