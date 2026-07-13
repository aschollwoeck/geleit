//! Pure helpers for turning IMAP envelope bytes into stored strings. Kept separate from the
//! network code in `imap.rs` so they stay unit- and mutation-tested.

/// Decode an IMAP envelope header byte string to a display `String`, or `None` if absent. Decodes
/// RFC 2047 "encoded-words" (`=?UTF-8?Q?...?=` / `=?...?B?...?=`) so non-ASCII subjects and names
/// read correctly (e.g. `=?UTF-8?Q?Gr=C3=BC=C3=9Fe?=` → `Grüße`). Plain values take a fast path.
pub(crate) fn decode_header(bytes: Option<&[u8]>) -> Option<String> {
    let bytes = bytes?;
    let raw = String::from_utf8_lossy(bytes);
    // No encoded-word marker → already a plain (lossy-UTF-8) value; skip the parse.
    if !raw.contains("=?") {
        return Some(raw.into_owned());
    }
    // Let mail-parser do the RFC 2047 decoding by parsing a synthetic header (encoded-words are
    // ASCII, so this is safe). Falls back to the lossy value if parsing somehow yields nothing.
    let mut doc = Vec::with_capacity(bytes.len() + 11);
    doc.extend_from_slice(b"Subject: ");
    doc.extend_from_slice(bytes);
    doc.extend_from_slice(b"\r\n\r\n");
    mail_parser::MessageParser::default()
        .parse(&doc)
        .and_then(|m| m.subject().map(str::to_owned))
        .or_else(|| Some(raw.into_owned()))
}

/// Build `(from_name, from_addr)` from an address's parts. `from_addr` is `mailbox@host` when both
/// are present, just `mailbox` if there is no host, or `None` if there is no mailbox.
pub(crate) fn address_parts(
    name: Option<&[u8]>,
    mailbox: Option<&[u8]>,
    host: Option<&[u8]>,
) -> (Option<String>, Option<String>) {
    let from_name = decode_header(name);
    let from_addr = match (decode_header(mailbox), decode_header(host)) {
        (Some(m), Some(h)) => Some(format!("{m}@{h}")),
        (Some(m), None) => Some(m),
        _ => None,
    };
    (from_name, from_addr)
}

/// How to name a sender: the display name if the envelope carried one, else the bare address, else a
/// calm placeholder. Pure.
///
/// The one definition — `geleit-app`'s `dto::display_sender` delegates here, so the message list and
/// a new-mail notification can never drift apart. It lives in the engine because a notification is
/// raised by the host worker, long before any DTO exists.
#[must_use]
pub fn display_sender(from_name: Option<&str>, from_addr: Option<&str>) -> String {
    let name = from_name.map(str::trim).filter(|s| !s.is_empty());
    let addr = from_addr.map(str::trim).filter(|s| !s.is_empty());
    name.or(addr).unwrap_or("(unknown sender)").to_owned()
}

/// The trailing (newest) sequence-number window of size `min(limit, exists)`, as inclusive 1-based
/// bounds `(start, end)`, or `None` when there is nothing to fetch. IMAP sequence numbers are
/// arrival-ordered, so the high end is the newest message.
pub(crate) fn recent_window(exists: u32, limit: u32) -> Option<(u32, u32)> {
    if exists == 0 || limit == 0 {
        return None;
    }
    let want = limit.min(exists);
    Some((exists - want + 1, exists))
}

#[cfg(test)]
mod tests {
    use super::{address_parts, decode_header, display_sender, recent_window};

    #[test]
    fn display_sender_prefers_the_name_then_the_address() {
        assert_eq!(display_sender(Some("Alice"), Some("a@x.io")), "Alice");
        assert_eq!(display_sender(None, Some("a@x.io")), "a@x.io");
        // A whitespace-only display name must not win over a real address (nor render as blank).
        assert_eq!(display_sender(Some("   "), Some("a@x.io")), "a@x.io");
        assert_eq!(display_sender(Some("  Alice  "), None), "Alice"); // trimmed
                                                                      // Nothing usable → a calm placeholder, never an empty string.
        assert_eq!(display_sender(None, None), "(unknown sender)");
        assert_eq!(display_sender(Some(" "), Some(" ")), "(unknown sender)");
    }

    #[test]
    fn recent_window_bounds() {
        assert_eq!(recent_window(0, 50), None); // empty mailbox
        assert_eq!(recent_window(5, 0), None); // limit 0
        assert_eq!(recent_window(1, 50), Some((1, 1))); // single message
        assert_eq!(recent_window(3, 50), Some((1, 3))); // fewer than limit → all
        assert_eq!(recent_window(10, 3), Some((8, 10))); // newest 3 of 10
        assert_eq!(recent_window(5, 5), Some((1, 5))); // exactly limit
    }

    #[test]
    fn decode_header_lossy_and_absent() {
        assert_eq!(decode_header(Some(b"Hello")), Some("Hello".to_owned()));
        assert_eq!(decode_header(None), None);
        // invalid UTF-8 is replaced, not dropped
        assert!(decode_header(Some(&[0xff, b'a'])).unwrap().contains('a'));
    }

    #[test]
    fn decode_header_rfc2047_encoded_words() {
        // Quoted-printable encoded-word (the form seen on real mail)
        assert_eq!(
            decode_header(Some(b"=?UTF-8?Q?Gr=C3=BC=C3=9Fe?=")),
            Some("Grüße".to_owned())
        );
        // the German bank subject from the bug report (Fwd: … – en-dash)
        assert_eq!(
            decode_header(Some(
                b"=?UTF-8?Q?Fwd=3A_Schnell_zum_Wunschkredit_=E2=80=93_jetzt?="
            )),
            Some("Fwd: Schnell zum Wunschkredit – jetzt".to_owned())
        );
        // Base64 encoded-word
        assert_eq!(
            decode_header(Some(b"=?UTF-8?B?w5xiZXI=?=")),
            Some("Über".to_owned())
        );
        // a plain value containing no encoded-word is returned unchanged
        assert_eq!(
            decode_header(Some(b"Plain subject")),
            Some("Plain subject".to_owned())
        );
    }

    #[test]
    fn address_parts_combines_mailbox_and_host() {
        let (name, addr) = address_parts(Some(b"Anna"), Some(b"anna"), Some(b"example.com"));
        assert_eq!(name.as_deref(), Some("Anna"));
        assert_eq!(addr.as_deref(), Some("anna@example.com"));
    }

    #[test]
    fn address_parts_handles_missing_pieces() {
        assert_eq!(
            address_parts(None, Some(b"a"), None),
            (None, Some("a".to_owned()))
        );
        assert_eq!(address_parts(None, None, Some(b"x")), (None, None));
        assert_eq!(
            address_parts(Some(b"N"), None, None),
            (Some("N".to_owned()), None)
        );
    }
}
