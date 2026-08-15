//! Cross-platform secret storage with an OS credential-manager implementation.

use std::{collections::HashMap, sync::RwLock};

use thiserror::Error;

/// Secure secret storage boundary.
pub trait SecretStore: Send + Sync {
    /// Stores or replaces a secret.
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError>;
    /// Retrieves a secret, returning `None` when it does not exist.
    fn get(&self, key: &str) -> Result<Option<String>, SecretError>;
    /// Deletes a secret if it exists.
    fn delete(&self, key: &str) -> Result<(), SecretError>;
}

/// Sanitized secret-store errors.
#[derive(Debug, Error)]
pub enum SecretError {
    /// The platform vault rejected an operation.
    #[error("the operating-system credential store is unavailable")]
    Platform,
    /// The in-memory test vault lock was poisoned.
    #[error("the secret store could not be accessed")]
    Synchronization,
}

/// Credential store backed by Keychain, Credential Manager, or Secret Service.
#[derive(Clone, Debug)]
pub struct OsSecretStore {
    service: String,
}

impl OsSecretStore {
    /// Creates an OS credential-store adapter with an application service name.
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(&self.service, key).map_err(|_| SecretError::Platform)
    }
}

impl SecretStore for OsSecretStore {
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.entry(key)?
            .set_password(value)
            .map_err(|_| SecretError::Platform)
    }

    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        match self.entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(SecretError::Platform),
        }
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(SecretError::Platform),
        }
    }
}

/// Deterministic non-persistent store for tests and previews.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    values: RwLock<HashMap<String, String>>,
}

impl SecretStore for MemorySecretStore {
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.values
            .write()
            .map_err(|_| SecretError::Synchronization)?
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        Ok(self
            .values
            .read()
            .map_err(|_| SecretError::Synchronization)?
            .get(key)
            .cloned())
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        self.values
            .write()
            .map_err(|_| SecretError::Synchronization)?
            .remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let store = MemorySecretStore::default();
        assert_eq!(store.get("token").unwrap(), None);
        store.set("token", "private").unwrap();
        assert_eq!(store.get("token").unwrap().as_deref(), Some("private"));
        store.delete("token").unwrap();
        assert_eq!(store.get("token").unwrap(), None);
    }
}
