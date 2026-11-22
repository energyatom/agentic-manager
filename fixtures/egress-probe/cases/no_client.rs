//! The control: uses reqwest without building a client, so clippy passes.
pub fn parse(text: &str) -> Option<reqwest::Url> {
    reqwest::Url::parse(text).ok()
}
