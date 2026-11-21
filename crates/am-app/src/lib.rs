//! `am-app`: the composition root (AD-32). It wires adapters to ports and
//! hosts the `agentic` CLI: `serve`, `validate` (story 1.4), `audit export`
//! and `audit verify` (story 1.7), the journaled spike run used by criterion
//! 2, and the `spike` subcommands for the owner's live runs. Since story
//! 1.13 it resolves each project's runtime folder outside the project
//! ([`runtime`]) through the OS identity adapters ([`identity`]). Story 2.1
//! adds `agentic accounts add|list|test|remove` ([`accounts`]), AD-15's
//! interim CLI Account edit, and the owner's real terminal ([`terminal`]).
#![forbid(unsafe_code)]

// The composition root depends on every adapter (Design Paradigm table); the
// crates the spike leaves empty are linked here so the graph stays as designed.
use am_api as _;
use am_connectors as _;
use am_extract as _;
use am_mcp as _;
use am_render as _;
use am_services as _;

pub mod accounts;
pub mod cli;
pub mod identity;
pub mod runtime;
pub mod spike;
pub mod terminal;

use std::sync::Arc;

use am_domain::{EgressClient, SecretStore};
use am_net::{EgressPolicy, NetEgress};

/// The Account id the spike reads the Anthropic key from (service `agentic-manager`).
pub const SPIKE_ANTHROPIC_ACCOUNT: &str = "spike-anthropic";

/// The Anthropic model the spike and `agentic accounts test` call when none
/// is named.
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-opus-5-5";

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
