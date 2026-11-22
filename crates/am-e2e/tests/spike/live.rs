//! Gates for live tests: they run only with `AM_LIVE=1` and skip, in words,
//! when their input is absent.

use std::sync::Arc;

use am_domain::net::{Caller, EgressDirectory, Endpoint, reserved_local_account};
use am_domain::{AuditEvent, AuditSink, EgressClient, HostPattern, HttpRequest, SecretStore};
use am_net::NetEgress;

/// The Keychain entry the spike's live runs read the Anthropic key from
/// (service `agentic-manager`). Story 2.2 removed `agentic spike c1` and
/// `c4` from the release binary (owner decision 9); the live tests keep
/// this id as their own.
pub const SPIKE_ANTHROPIC_ACCOUNT: &str = "spike-anthropic";

pub fn live_enabled() -> bool {
    std::env::var("AM_LIVE").map(|v| v == "1").unwrap_or(false)
}

pub fn skip(why: &str) {
    eprintln!("skipped: {why}");
}

/// The live tests' egress directory (story 2.2): the spike's Anthropic
/// Account on Anthropic's endpoint and the reserved local Accounts; no
/// seat, no MCP server.
struct SpikeDirectory;

impl EgressDirectory for SpikeDirectory {
    fn seat_hosts(&self, _seat: &str) -> Option<Vec<HostPattern>> {
        None
    }
    fn account_endpoint(&self, account: &str) -> Option<Endpoint> {
        if account == SPIKE_ANTHROPIC_ACCOUNT {
            return Some(Endpoint {
                scheme: "https".into(),
                host: am_providers::anthropic::ANTHROPIC_HOST.into(),
                port: 443,
            });
        }
        reserved_local_account(account)
    }
    fn mcp_endpoint(&self, _server: &str) -> Option<Endpoint> {
        None
    }
}

/// Blocks are printed: a live run has no project store.
struct PrintAudit;

impl AuditSink for PrintAudit {
    fn record(&self, e: AuditEvent) {
        eprintln!("audit {} {} {} {:?}", e.action, e.target, e.outcome, e.args);
    }
}

/// The real egress client over the Keychain.
pub fn egress() -> Arc<dyn EgressClient> {
    Arc::new(NetEgress::new(
        Arc::new(SpikeDirectory),
        am_app::keychain(),
        Arc::new(PrintAudit),
    ))
}

pub fn anthropic_key_present() -> bool {
    matches!(
        am_secrets::KeychainStore::new().get(SPIKE_ANTHROPIC_ACCOUNT),
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
        .send(HttpRequest {
            caller: Some(Caller::Account {
                account: "local-lmstudio".into(),
                seat: None,
            }),
            ..HttpRequest::get("http://127.0.0.1:1234/v1/models")
        })
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
