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

/// The LM Studio model named by `AM_SPIKE_LOCAL_MODEL` (never guessed: the
/// first listed model may be an embedding or text-only model), once LM Studio
/// answers. `Err` is the skip reason.
pub async fn lm_studio_model() -> Result<String, String> {
    let model = std::env::var("AM_SPIKE_LOCAL_MODEL").map_err(|_| {
        "AM_SPIKE_LOCAL_MODEL not set: name the LM Studio model (tool-capable for C1, vision-capable for C4)".to_string()
    })?;
    let resp = egress()
        .send(HttpRequest::get("http://127.0.0.1:1234/v1/models"))
        .await
        .map_err(|_| "LM Studio is not answering on 127.0.0.1:1234".to_string())?;
    if resp.status != 200 {
        return Err(format!("LM Studio answered HTTP {}", resp.status));
    }
    Ok(model)
}

pub fn anthropic_model() -> String {
    std::env::var("AM_SPIKE_ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-opus-5-5".into())
}
