//! Builds a reqwest client with `Client::new()` outside am-net.
pub fn client() -> reqwest::Client {
    reqwest::Client::new()
}
