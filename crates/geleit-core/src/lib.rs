//! `geleit-core` — UI-agnostic domain types shared across the engine.
//!
//! Scaffold placeholder (slice S0.1). Real domain types (Account, Mailbox, Message, …)
//! arrive in later slices. This crate must never depend on UI code (constitution P4,
//! ADR-0003).

/// Returns `true` if `addr` is a syntactically plausible email address.
///
/// Placeholder validation for the scaffold; real address/MIME handling comes later.
/// Kept deliberately small so it is a meaningful mutation-testing target.
#[must_use]
pub fn looks_like_email(addr: &str) -> bool {
    match addr.find('@') {
        Some(at) => at > 0 && at < addr.len() - 1 && !addr.contains(' '),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::looks_like_email;

    #[test]
    fn accepts_plausible_address() {
        assert!(looks_like_email("user@example.com"));
    }

    #[test]
    fn rejects_without_at() {
        assert!(!looks_like_email("not-an-email"));
    }

    #[test]
    fn rejects_at_string_edges() {
        assert!(!looks_like_email("@example.com"));
        assert!(!looks_like_email("user@"));
    }

    #[test]
    fn rejects_addresses_with_spaces() {
        assert!(!looks_like_email("user name@example.com"));
    }
}
