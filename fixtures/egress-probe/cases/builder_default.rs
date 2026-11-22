//! Builds a reqwest client from `ClientBuilder::default()` outside am-net.
pub fn builder() -> reqwest::ClientBuilder {
    reqwest::ClientBuilder::default()
}
