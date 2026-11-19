//! Secrets port (AD-17). `SecretString` implements no Debug, Display or
//! Serialize, and is materialised only inside `am-net` request building.

/// A secret value. Deliberately has no `Debug`, `Display`, `Clone` or serde impls.
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: String) -> Self {
        SecretString(value)
    }
    /// Expose the value. Call sites: `am-net` request building and the keychain adapter only.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        // Best effort without `unsafe`: overwrite the buffer we own.
        let len = self.0.len();
        self.0.clear();
        self.0.extend(std::iter::repeat_n('\0', len));
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecretError {
    #[error("the OS keychain is not supported on this platform yet")]
    NotSupported,
    #[error("keychain error: {0}")]
    Store(String),
}

/// Keys live under service `agentic-manager` and account `<account-id>`.
pub trait SecretStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<SecretString>, SecretError>;
    fn set(&self, account: &str, secret: SecretString) -> Result<(), SecretError>;
    fn delete(&self, account: &str) -> Result<(), SecretError>;
}
