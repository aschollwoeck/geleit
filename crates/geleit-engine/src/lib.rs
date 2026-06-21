//! `geleit-engine` — UI-agnostic engine facade.
//!
//! Scaffold placeholder (slice S0.1). This crate will grow into the store, sync, MIME,
//! search, transport, and auth subsystems. It depends on [`geleit_core`] and must never
//! depend on UI code (constitution P4, ADR-0003).

mod envelope;
pub mod imap;
mod mime;
pub mod safehtml;
mod sync;
pub mod thread;

use geleit_core::looks_like_email;
use geleit_platform::secret::{SecretError, SecretStore};

/// Returns `true` if an account with this address is usable by the engine.
///
/// Placeholder that delegates to [`geleit_core`]; real account/sync logic arrives in M1+.
#[must_use]
pub fn can_use_account(address: &str) -> bool {
    looks_like_email(address)
}

/// Records, via the platform [`SecretStore`] seam, that an account address has been configured.
///
/// Placeholder demonstrating the engine consuming the platform abstraction (S0.5); real
/// credential and at-rest-key handling arrive in M1+.
pub fn store_account_marker(store: &dyn SecretStore, address: &str) -> Result<(), SecretError> {
    store.set("geleit", address, b"configured")
}

#[cfg(test)]
mod tests {
    use super::{can_use_account, store_account_marker};
    use geleit_platform::secret::{InMemorySecretStore, SecretStore};

    #[test]
    fn accepts_usable_account() {
        assert!(can_use_account("user@example.com"));
    }

    #[test]
    fn rejects_unusable_account() {
        assert!(!can_use_account("nope"));
    }

    #[test]
    fn stores_account_marker_via_seam() {
        let store = InMemorySecretStore::new();
        store_account_marker(&store, "a@example.com").unwrap();
        assert_eq!(
            store.get("geleit", "a@example.com").unwrap().as_deref(),
            Some(&b"configured"[..])
        );
    }
}
