//! `am-secrets`: the `SecretStore` adapter (AD-17, AD-47).
//!
//! v1 has one store, the macOS Keychain (keyring-core + the Apple native
//! store, the keyring 4 family). Keys live under service `agentic-manager`
//! and account `<account-id>`. On other platforms the store compiles and
//! answers "not supported on this platform yet".
#![forbid(unsafe_code)]

pub use am_domain::{SecretError, SecretStore, SecretString};

/// The keychain service every key is stored under.
pub const SERVICE: &str = "agentic-manager";

/// The OS keychain store.
pub struct KeychainStore {
    service: String,
}

impl KeychainStore {
    pub fn new() -> Self {
        KeychainStore {
            service: SERVICE.to_string(),
        }
    }

    /// A store under another service name (tests only use `SERVICE`; kept for
    /// the canary leak scan's isolation).
    pub fn with_service(service: &str) -> Self {
        KeychainStore {
            service: service.to_string(),
        }
    }
}

impl Default for KeychainStore {
    fn default() -> Self {
        KeychainStore::new()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use keyring_core::api::CredentialStoreApi;

    fn entry(service: &str, account: &str) -> Result<keyring_core::Entry, SecretError> {
        let store = apple_native_keyring_store::keychain::Store::new()
            .map_err(|e| SecretError::Store(e.to_string()))?;
        store
            .build(service, account, None)
            .map_err(|e| SecretError::Store(e.to_string()))
    }

    impl SecretStore for KeychainStore {
        fn get(&self, account: &str) -> Result<Option<SecretString>, SecretError> {
            match entry(&self.service, account)?.get_password() {
                Ok(v) => Ok(Some(SecretString::new(v))),
                Err(keyring_core::Error::NoEntry) => Ok(None),
                Err(e) => Err(SecretError::Store(e.to_string())),
            }
        }
        fn set(&self, account: &str, secret: SecretString) -> Result<(), SecretError> {
            entry(&self.service, account)?
                .set_password(secret.expose())
                .map_err(|e| SecretError::Store(e.to_string()))
        }
        fn delete(&self, account: &str) -> Result<(), SecretError> {
            match entry(&self.service, account)?.delete_credential() {
                Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
                Err(e) => Err(SecretError::Store(e.to_string())),
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;

    impl SecretStore for KeychainStore {
        fn get(&self, _account: &str) -> Result<Option<SecretString>, SecretError> {
            Err(SecretError::NotSupported)
        }
        fn set(&self, _account: &str, _secret: SecretString) -> Result<(), SecretError> {
            Err(SecretError::NotSupported)
        }
        fn delete(&self, _account: &str) -> Result<(), SecretError> {
            Err(SecretError::NotSupported)
        }
    }
}
