//! Gates for live tests: they run only with `AM_LIVE=1` and skip, in words,
//! when their input is absent.

use std::sync::Arc;

use am_domain::{EgressClient, HttpRequest, SecretStore};

pub fn live_enabled() -> bool {
    std::env::var("AM_LIVE").map(|v| v == "1").unwrap_or(false)
}

pub fn skip(why: &str) {
    eprintln!("skipped: {why}");
}

/// The real egress client over the Keychain.
pub fn egress() -> Arc<dyn EgressClient> {
    am_app::default_egress(am_app::keychain())
}

pub fn anthropic_key_present() -> bool {
    matches!(
        am_secrets::KeychainStore::new().get(am_app::SPIKE_ANTHROPIC_ACCOUNT),
        Ok(Some(_))
    )
}

pub async fn lm_studio_model() -> Option<String> {
    if let Ok(m) = std::env::var("AM_SPIKE_LOCAL_MODEL") {
        return Some(m);
    }
    let resp = egress()
        .send(HttpRequest::get("http://127.0.0.1:1234/v1/models"))
        .await
        .ok()?;
    let body = am_net::read_all(resp.body.as_ref()).await.ok()?;
    let text = String::from_utf8_lossy(&body).to_string();
    let i = text.find("\"id\"")?;
    let rest = &text[i + 4..];
    let a = rest.find('"')? + 1;
    let b = rest[a..].find('"')? + a;
    Some(rest[a..b].to_string())
}

pub fn anthropic_model() -> String {
    std::env::var("AM_SPIKE_ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-opus-5-5".into())
}
