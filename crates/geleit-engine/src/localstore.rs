//! Opening the **encrypted** local store (SEC-1, ADR-0008) — the one bootstrap both UIs share.
//!
//! Moved here from the Slint app in S9.1: it is UI-agnostic (keychain → SQLCipher key → open), and
//! the Tauri shell (M9) needs the exact same logic. Duplicating it would be a genuine hazard —
//! [`db_key`]'s refusal to overwrite an existing key is what stops a transient keychain failure from
//! discarding the real key and bricking the user's mailbox.
use geleit_platform::secret::SecretStore;
use geleit_store::Store;

const DB_KEY_SERVICE: &str = "geleit-db";
const DB_KEY_ACCOUNT: &str = "key";
/// SQLCipher key length (256-bit).
const KEY_LEN: usize = 32;

/// The database encryption key: fetched from the keychain, or a fresh 32-byte random key generated
/// and stored there on first run. Never logged (P2).
///
/// A key is generated **only** when the keychain reports the entry genuinely *absent*. A read error,
/// or a present-but-wrong-size key, is surfaced instead — never overwritten. Overwriting would
/// silently discard the real key and leave the encrypted database permanently unopenable.
pub fn db_key(secrets: &dyn SecretStore) -> Result<Vec<u8>, String> {
    match secrets.get(DB_KEY_SERVICE, DB_KEY_ACCOUNT) {
        Ok(Some(key)) if key.len() == KEY_LEN => return Ok(key),
        Ok(Some(_)) => return Err("The stored encryption key looks corrupt.".to_owned()),
        Ok(None) => {} // first run → generate below
        Err(_) => return Err("Couldn't read the encryption key from the keychain.".to_owned()),
    }
    let mut key = vec![0u8; KEY_LEN];
    getrandom::fill(&mut key).map_err(|_| "Couldn't generate an encryption key.".to_owned())?;
    secrets
        .set(DB_KEY_SERVICE, DB_KEY_ACCOUNT, &key)
        .map_err(|_| "Couldn't store the encryption key.".to_owned())?;
    Ok(key)
}

/// Open the **encrypted** local store, fetching (or creating) its key from the keychain.
pub fn open_store(db_path: &str, secrets: &dyn SecretStore) -> Result<Store, String> {
    let key = db_key(secrets)?;
    Store::open_encrypted(db_path, &key)
        .map_err(|_| "Couldn't open the encrypted mailbox.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use geleit_platform::secret::{InMemorySecretStore, SecretError};

    #[test]
    fn generates_and_persists_a_key_on_first_run() {
        let secrets = InMemorySecretStore::new();
        let key = db_key(&secrets).expect("first run generates a key");
        assert_eq!(key.len(), KEY_LEN);
        // ...and the SAME key comes back next time, rather than a fresh one.
        assert_eq!(db_key(&secrets).expect("second run"), key);
    }

    #[test]
    fn returns_the_existing_key() {
        let secrets = InMemorySecretStore::new();
        let stored = vec![7u8; KEY_LEN];
        secrets
            .set(DB_KEY_SERVICE, DB_KEY_ACCOUNT, &stored)
            .unwrap();
        assert_eq!(db_key(&secrets).unwrap(), stored);
    }

    /// The brick-the-mailbox guard: a corrupt key must be *reported*, never silently replaced.
    #[test]
    fn refuses_to_overwrite_a_wrong_size_key() {
        let secrets = InMemorySecretStore::new();
        secrets
            .set(DB_KEY_SERVICE, DB_KEY_ACCOUNT, b"too-short")
            .unwrap();
        assert!(db_key(&secrets).is_err());
        // the original bytes are still there — we did not clobber them
        assert_eq!(
            secrets
                .get(DB_KEY_SERVICE, DB_KEY_ACCOUNT)
                .unwrap()
                .unwrap(),
            b"too-short"
        );
    }

    /// A keychain that fails to *read* must not be treated as "no key yet" — that would generate a
    /// new key over a perfectly good one.
    #[test]
    fn refuses_to_generate_when_the_keychain_read_fails() {
        struct FailingRead;
        impl SecretStore for FailingRead {
            fn get(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, SecretError> {
                Err(SecretError::Backend("keychain locked".into()))
            }
            fn set(&self, _: &str, _: &str, _: &[u8]) -> Result<(), SecretError> {
                panic!("must not write a key when the read failed");
            }
            fn delete(&self, _: &str, _: &str) -> Result<(), SecretError> {
                unreachable!()
            }
        }
        assert!(db_key(&FailingRead).is_err());
    }
}
