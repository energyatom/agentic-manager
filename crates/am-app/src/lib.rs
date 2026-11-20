//! `am-app`: the composition root (AD-32). It wires adapters to ports and
//! hosts the `agentic` CLI: `serve`, `validate` (story 1.4), the journaled
//! spike run used by criterion 2, and the `spike` subcommands for the owner's
//! live runs.
#![forbid(unsafe_code)]

// The composition root depends on every adapter (Design Paradigm table); the
// crates the spike leaves empty are linked here so the graph stays as designed.
use am_api as _;
use am_connectors as _;
use am_extract as _;
use am_mcp as _;
use am_render as _;
use am_services as _;

pub mod cli;
pub mod spike;

use std::sync::Arc;

use am_domain::{EgressClient, SecretStore};
use am_net::{EgressPolicy, NetEgress};

/// The Account id the spike reads the Anthropic key from (service `agentic-manager`).
pub const SPIKE_ANTHROPIC_ACCOUNT: &str = "spike-anthropic";

/// The production egress client: Anthropic's host plus the local Account ports.
pub fn default_egress(secrets: Arc<dyn SecretStore>) -> Arc<dyn EgressClient> {
    let policy = EgressPolicy {
        allowed_hosts: vec![am_providers::anthropic::ANTHROPIC_HOST.to_string()],
        ..EgressPolicy::default()
    };
    Arc::new(NetEgress::new(policy, secrets, None))
}

/// The OS keychain store.
pub fn keychain() -> Arc<dyn SecretStore> {
    Arc::new(am_secrets::KeychainStore::new())
}
