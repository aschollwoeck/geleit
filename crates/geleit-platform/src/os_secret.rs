//! OS keychain–backed [`SecretStore`] (Linux Secret Service via `keyring`'s pure-Rust zbus backend).
//! This is the real credential store the app uses (replacing [`InMemorySecretStore`] from M1).
//!
//! External integration; excluded from mutation testing (like the engine's `imap.rs`) and verified
//! by a live `#[ignore]` round-trip test. Errors deliberately carry **no** secret or account
//! material (constitution P2, guidelines §4/§9).
//!
//! [`InMemorySecretStore`]: crate::secret::InMemorySecretStore

use crate::secret::{SecretError, SecretStore};

/// A [`SecretStore`] backed by the OS keychain. Stateless: each call opens a keyring entry for
/// `(service, account)`. `Send + Sync` (a unit struct), so it can be shared across threads.
#[derive(Debug, Default)]
pub struct OsSecretStore;

impl OsSecretStore {
    /// Create a keychain-backed secret store.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn entry(service: &str, account: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(service, account)
            .map_err(|_| SecretError::Backend("keychain unavailable".to_owned()))
    }
}

impl SecretStore for OsSecretStore {
    fn set(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), SecretError> {
        Self::entry(service, account)?
            .set_secret(secret)
            .map_err(|_| SecretError::Backend("could not store secret".to_owned()))
    }

    fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, SecretError> {
        classify_get(Self::entry(service, account)?.get_secret())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError> {
        classify_delete(Self::entry(service, account)?.delete_credential())
    }
}

/// Map a keyring `get_secret` result: a missing entry is `None`; any other failure is an error
/// (never silently `None` — that would hide a locked/unreadable keychain). Pure → unit-tested.
fn classify_get(result: Result<Vec<u8>, keyring::Error>) -> Result<Option<Vec<u8>>, SecretError> {
    match result {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(SecretError::Backend("could not read secret".to_owned())),
    }
}

/// Map a keyring `delete_credential` result: deleting an absent entry succeeds (idempotent); any
/// other failure is an error. Pure → unit-tested.
fn classify_delete(result: Result<(), keyring::Error>) -> Result<(), SecretError> {
    match result {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(SecretError::Backend("could not delete secret".to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_delete, classify_get, OsSecretStore};
    use crate::secret::SecretStore;

    #[test]
    fn classify_get_maps_arms() {
        assert_eq!(
            classify_get(Ok(b"x".to_vec())).unwrap(),
            Some(b"x".to_vec())
        );
        assert_eq!(classify_get(Err(keyring::Error::NoEntry)).unwrap(), None);
        // any non-NoEntry failure (e.g. a locked keychain) is an error, never silent None
        assert!(classify_get(Err(keyring::Error::NoDefaultStore)).is_err());
    }

    #[test]
    fn classify_delete_maps_arms() {
        assert!(classify_delete(Ok(())).is_ok());
        assert!(classify_delete(Err(keyring::Error::NoEntry)).is_ok()); // idempotent
        assert!(classify_delete(Err(keyring::Error::NoDefaultStore)).is_err());
    }

    /// Round-trip against the real OS keychain. Needs a secret service (gnome-keyring / KWallet) on
    /// the session bus, so it's `#[ignore]`d (CI has none); run with `-- --ignored`.
    #[test]
    #[ignore = "requires an OS secret service on the session bus"]
    fn os_keychain_roundtrip() {
        let store = OsSecretStore::new();
        let (service, account) = ("geleit-test-service", "geleit-test-account");
        let _ = store.delete(service, account); // start clean

        assert_eq!(store.get(service, account).unwrap(), None);
        store.set(service, account, b"hunter2").unwrap();
        assert_eq!(
            store.get(service, account).unwrap(),
            Some(b"hunter2".to_vec())
        );
        store.set(service, account, b"changed").unwrap(); // overwrite
        assert_eq!(
            store.get(service, account).unwrap(),
            Some(b"changed".to_vec())
        );
        store.delete(service, account).unwrap();
        assert_eq!(store.get(service, account).unwrap(), None);
        store.delete(service, account).unwrap(); // idempotent
    }
}
