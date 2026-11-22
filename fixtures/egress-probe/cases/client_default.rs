//! Builds a reqwest client with `Client::default()` outside am-net.
pub fn client() -> reqwest::Client {
    reqwest::Client::default()
}
