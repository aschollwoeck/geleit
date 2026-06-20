//! `geleit-engine` — UI-agnostic engine facade.
//!
//! Scaffold placeholder (slice S0.1). This crate will grow into the store, sync, MIME,
//! search, transport, and auth subsystems. It depends on [`geleit_core`] and must never
//! depend on UI code (constitution P4, ADR-0003).

use geleit_core::looks_like_email;

/// Returns `true` if an account with this address is usable by the engine.
///
/// Placeholder that delegates to [`geleit_core`]; real account/sync logic arrives in M1+.
#[must_use]
pub fn can_use_account(address: &str) -> bool {
    looks_like_email(address)
}

#[cfg(test)]
mod tests {
    use super::can_use_account;

    #[test]
    fn accepts_usable_account() {
        assert!(can_use_account("user@example.com"));
    }

    #[test]
    fn rejects_unusable_account() {
        assert!(!can_use_account("nope"));
    }
}
