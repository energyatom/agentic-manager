//! Calls `reqwest::Client::default()` in a body; no signature names the type.
pub fn make() {
    let _ = reqwest::Client::default();
}
