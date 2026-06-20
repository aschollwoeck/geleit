//! Pure helpers for turning IMAP envelope bytes into stored strings. Kept separate from the
//! network code in `imap.rs` so they stay unit- and mutation-tested. RFC2047 MIME-word decoding
//! of headers is deferred to the MIME slice (S1.6); this is the naive lossy-UTF-8 pass.

/// Decode an IMAP envelope header byte string to a `String` (lossy UTF-8), or `None` if absent.
pub(crate) fn decode_header(bytes: Option<&[u8]>) -> Option<String> {
    bytes.map(|b| String::from_utf8_lossy(b).into_owned())
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
    use super::{address_parts, decode_header, recent_window};

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
