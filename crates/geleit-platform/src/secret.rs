//! Secret-storage seam — credentials, OAuth tokens, and the at-rest encryption key live in the
//! OS keychain. Real backends (later milestones): Secret Service / libsecret (Linux), Keychain
//! Services (macOS), Credential Manager (Windows).

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

/// Error from a [`SecretStore`] operation.
#[derive(Debug)]
pub enum SecretError {
    /// The backend failed or was unavailable (e.g. the keychain is locked).
    Backend(String),
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretError::Backend(msg) => write!(f, "secret store backend error: {msg}"),
        }
    }
}

impl std::error::Error for SecretError {}

/// Stores and retrieves secrets keyed by `(service, account)`.
///
/// Implementations must never log secret material (constitution P2, guidelines §4/§9). Real OS
/// backends are added in later milestones (M1: the at-rest key; M7: OAuth tokens).
///
/// Note: the secret byte type (`&[u8]` / `Vec<u8>`) is **provisional** — it becomes a zeroizing
/// wrapper when `zeroize` lands in M1 (guidelines §9). See ADR-0004.
pub trait SecretStore: Send + Sync {
    /// Store `secret` under `(service, account)`, overwriting any existing value.
    fn set(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), SecretError>;
    /// Fetch the secret for `(service, account)`, or `None` if absent.
    fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, SecretError>;
    /// Remove the secret for `(service, account)`; succeeds even if absent.
    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError>;
}

/// In-memory [`SecretStore`] for tests and early development.
///
/// **Not secure** — secrets live unencrypted in process memory. Never use in a release build; it
/// exists only so engine code depending on the seam can be written and tested before the real OS
/// keychain backends land.
#[derive(Default)]
pub struct InMemorySecretStore {
    map: Mutex<HashMap<(String, String), Vec<u8>>>,
}

impl InMemorySecretStore {
    /// Create an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for InMemorySecretStore {
    fn set(&self, service: &str, account: &str, secret: &[u8]) -> Result<(), SecretError> {
        self.map
            .lock()
            .expect("secret store mutex poisoned")
            .insert((service.to_owned(), account.to_owned()), secret.to_vec());
        Ok(())
    }

    fn get(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>, SecretError> {
        Ok(self
            .map
            .lock()
            .expect("secret store mutex poisoned")
            .get(&(service.to_owned(), account.to_owned()))
            .cloned())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), SecretError> {
        self.map
            .lock()
            .expect("secret store mutex poisoned")
            .remove(&(service.to_owned(), account.to_owned()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{InMemorySecretStore, SecretError, SecretStore};

    #[test]
    fn error_displays_message() {
        let e = SecretError::Backend("keychain locked".to_owned());
        let s = e.to_string();
        assert!(s.contains("keychain locked"), "{s}");
        assert!(s.contains("secret store"), "{s}");
    }

    #[test]
    fn set_then_get_round_trips() {
        let s = InMemorySecretStore::new();
        s.set("geleit", "a@example.com", b"token").unwrap();
        assert_eq!(
            s.get("geleit", "a@example.com").unwrap().as_deref(),
            Some(&b"token"[..])
        );
    }

    #[test]
    fn get_missing_is_none() {
        let s = InMemorySecretStore::new();
        assert!(s.get("geleit", "missing").unwrap().is_none());
    }

    #[test]
    fn set_overwrites_existing() {
        let s = InMemorySecretStore::new();
        s.set("geleit", "a", b"one").unwrap();
        s.set("geleit", "a", b"two").unwrap();
        assert_eq!(s.get("geleit", "a").unwrap().as_deref(), Some(&b"two"[..]));
    }

    #[test]
    fn delete_removes_secret() {
        let s = InMemorySecretStore::new();
        s.set("geleit", "a", b"x").unwrap();
        s.delete("geleit", "a").unwrap();
        assert!(s.get("geleit", "a").unwrap().is_none());
    }

    #[test]
    fn delete_absent_is_ok() {
        let s = InMemorySecretStore::new();
        assert!(s.delete("geleit", "nope").is_ok());
    }
}
