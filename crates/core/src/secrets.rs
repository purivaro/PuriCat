//! Secret storage. Passwords never touch the settings database, the logs or AI prompts.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::{Error, Result};

/// Keychain "service" name. Changing it orphans every saved password.
pub const SERVICE: &str = "PuriCat";

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}

/// Key under which a connection's database password is stored.
pub fn password_key(connection_id: &str) -> String {
    format!("connection/{connection_id}/password")
}

/// macOS Keychain / Windows Credential Manager.
pub struct OsKeychain;

impl OsKeychain {
    fn entry(key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, key).map_err(|e| Error::Secret(e.to_string()))
    }
}

impl SecretStore for OsKeychain {
    fn get(&self, key: &str) -> Result<Option<String>> {
        match Self::entry(key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(Error::Secret(e.to_string())),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        Self::entry(key)?
            .set_password(value)
            .map_err(|e| Error::Secret(e.to_string()))
    }

    fn delete(&self, key: &str) -> Result<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(Error::Secret(e.to_string())),
        }
    }
}

/// Process-local store for tests and keychain-free development runs.
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let s = MemoryStore::default();
        let k = password_key("abc");
        assert_eq!(s.get(&k).unwrap(), None);
        s.set(&k, "p@ss wörd ไทย").unwrap();
        assert_eq!(s.get(&k).unwrap().as_deref(), Some("p@ss wörd ไทย"));
        s.delete(&k).unwrap();
        s.delete(&k).unwrap(); // deleting twice is fine
        assert_eq!(s.get(&k).unwrap(), None);
    }

    /// Uses the real macOS Keychain / Windows Credential Manager under a throwaway key.
    #[test]
    #[ignore = "touches the real OS credential store; run with `cargo test -- --ignored`"]
    fn os_keychain_round_trip() {
        let store = OsKeychain;
        let key = password_key(&format!("selftest-{}", uuid::Uuid::new_v4()));
        assert_eq!(store.get(&key).unwrap(), None);
        store.set(&key, "pässword ไทย").unwrap();
        assert_eq!(store.get(&key).unwrap().as_deref(), Some("pässword ไทย"));
        store.set(&key, "changed").unwrap();
        assert_eq!(store.get(&key).unwrap().as_deref(), Some("changed"));
        store.delete(&key).unwrap();
        assert_eq!(store.get(&key).unwrap(), None);
        store.delete(&key).unwrap();
    }
}
