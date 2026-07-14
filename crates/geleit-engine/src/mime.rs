//! Pure MIME-body parsing via `mail-parser`. Kept separate from the network code in `imap.rs` so
//! it stays unit- and mutation-tested. Turns raw RFC822 bytes into the plaintext/HTML body, a
//! short snippet, and an attachment flag.

use mail_parser::{MessageParser, MimeHeaders};

/// Metadata for one attachment (the bytes aren't stored yet — viewing only, READ-8).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Attachment {
    pub filename: Option<String>,
    pub content_type: String,
    pub size: u64,
}

/// The parsed pieces of a message body we store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ParsedBody {
    pub plain: Option<String>,
    pub html: Option<String>,
    pub snippet: Option<String>,
    pub has_attachments: bool,
    pub attachments: Vec<Attachment>,
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
    let attachments: Vec<Attachment> = msg
        .attachments()
        .map(|part| Attachment {
            filename: part.attachment_name().map(str::to_owned),
            content_type: part_content_type(part),
            size: part.len() as u64,
        })
        .collect();
    ParsedBody {
        plain,
        html,
        snippet,
        has_attachments: !attachments.is_empty(),
        attachments,
    }
}

/// A part's `type/subtype` (e.g. `application/pdf`), defaulting to `application/octet-stream`.
fn part_content_type(part: &mail_parser::MessagePart) -> String {
    part.content_type().map_or_else(
        || "application/octet-stream".to_owned(),
        |ct| match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub),
            None => ct.ctype().to_owned(),
        },
    )
}

/// One attachment's actual bytes, pulled from a raw message by its position (0-based) among the
/// attachments — the same order [`parse_body`] lists them, so a reading-pane index maps straight
/// here. `None` if the message can't be parsed or has no attachment at that index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtractedAttachment {
    pub filename: Option<String>,
    pub content_type: String,
    pub data: Vec<u8>,
}

pub(crate) fn extract_attachment(raw: &[u8], index: usize) -> Option<ExtractedAttachment> {
    let msg = MessageParser::default().parse(raw)?;
    let part = msg.attachments().nth(index)?;
    Some(ExtractedAttachment {
        filename: part.attachment_name().map(str::to_owned),
        content_type: part_content_type(part),
        data: part.contents().to_vec(),
    })
}

/// A short one-line preview: whitespace collapsed to single spaces, truncated to `max` characters.
pub(crate) fn make_snippet(text: &str, max: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max).collect()
}

#[cfg(test)]
mod html_only_tests {
    use super::parse_body;

    /// Webmail composes HTML, so a draft started there usually has **no** `text/plain` part. The whole
    /// "continuing it keeps every word, drops the styling" promise — and, crucially, the safety of
    /// expunging the original once the continued draft is saved — rests on `plain` still being
    /// populated for such a message. It is, because `mail_parser`'s `body_text` converts an HTML part
    /// to text. Nothing else in this repo asserts that, and "take only genuine text/plain parts" is
    /// exactly the sort of tightening someone would plausibly make (we did it for `html`, just above).
    /// That edit would open every webmail draft empty and then destroy the original on save.
    #[test]
    fn an_html_only_message_still_yields_a_plain_body() {
        let raw = b"Subject: From webmail\r\nContent-Type: text/html; charset=utf-8\r\n\r\n\
                    <html><body><p>The roofer can come <b>Thursday</b>.</p></body></html>";
        let parsed = parse_body(raw);
        let plain = parsed
            .plain
            .expect("an HTML-only message must still give us text");
        assert!(
            plain.contains("The roofer can come") && plain.contains("Thursday"),
            "the words must survive the HTML → text conversion: {plain:?}"
        );
        assert!(
            parsed.html.is_some(),
            "…and it's still known to be formatted"
        );
    }
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
    fn extract_attachment_returns_the_right_part_bytes() {
        use super::extract_attachment;
        // Index 0 is the message's single attachment: name + decoded bytes match the fixture.
        let a = extract_attachment(MULTIPART, 0).expect("attachment 0");
        assert_eq!(a.filename.as_deref(), Some("note.txt"));
        assert_eq!(a.data, b"attached file contents");
        assert!(a.content_type.starts_with("text/plain"));
        // Out-of-range index → None (not a panic).
        assert!(extract_attachment(MULTIPART, 1).is_none());
        // Garbage input → None.
        assert!(extract_attachment(b"not a message", 0).is_none());
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
    fn extracts_attachment_metadata() {
        let b = parse_body(MULTIPART);
        assert!(b.has_attachments);
        assert_eq!(b.attachments.len(), 1);
        let a = &b.attachments[0];
        assert_eq!(a.filename.as_deref(), Some("note.txt"));
        assert_eq!(a.content_type, "text/plain");
        assert!(a.size > 0, "size {}", a.size);
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
