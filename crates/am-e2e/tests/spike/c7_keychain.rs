//! Criterion 7: store, read and delete a key through `SecretStore` in the
//! macOS Keychain (service `agentic-manager`), with a canary value. Off macOS
//! the store compiles and answers "not supported on this platform yet".

use am_domain::{SecretError, SecretStore, SecretString};
use am_secrets::{KeychainStore, SERVICE};

#[test]
fn service_name_is_agentic_manager() {
    assert_eq!(SERVICE, "agentic-manager");
}

#[cfg(target_os = "macos")]
#[test]
fn keychain_store_read_delete_with_canary() {
    let store = KeychainStore::new();
    let account = format!(
        "spike-test-canary-{}-{}",
        std::process::id(),
        jiff::Timestamp::now().as_millisecond()
    );
    let canary = "am-canary-c7-3b9f1e";
    assert!(store.get(&account).unwrap().is_none());
    store
        .set(&account, SecretString::new(canary.into()))
        .unwrap();
    let got = store.get(&account).unwrap().expect("stored");
    assert_eq!(got.expose(), canary);
    // Overwrite keeps one entry.
    store
        .set(&account, SecretString::new(format!("{canary}-2")))
        .unwrap();
    assert_eq!(
        store.get(&account).unwrap().unwrap().expose(),
        format!("{canary}-2")
    );
    store.delete(&account).unwrap();
    assert!(store.get(&account).unwrap().is_none());
    // Deleting an absent key is not an error.
    store.delete(&account).unwrap();
}

#[cfg(not(target_os = "macos"))]
#[test]
fn keychain_is_not_supported_off_macos() {
    let store = KeychainStore::new();
    assert_eq!(store.get("x").err(), Some(SecretError::NotSupported));
    assert_eq!(
        SecretError::NotSupported.to_string(),
        "the OS keychain is not supported on this platform yet"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn not_supported_error_is_worded() {
    assert_eq!(
        SecretError::NotSupported.to_string(),
        "the OS keychain is not supported on this platform yet"
    );
}
