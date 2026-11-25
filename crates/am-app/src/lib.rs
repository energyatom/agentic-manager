//! `am-app`: the composition root (AD-32). It wires adapters to ports and
//! hosts the `agentic` CLI: `serve`, `validate` (story 1.4), `audit export`
//! and `audit verify` (story 1.7), the journaled spike run used by criterion
//! 2, and the `spike` subcommands for the owner's live runs. Since story
//! 1.13 it resolves each project's runtime folder outside the project
//! ([`runtime`]) through the OS identity adapters ([`identity`]). Story 2.1
//! adds `agentic accounts add|list|test|remove` ([`accounts`]), AD-15's
//! interim CLI Account edit, and the owner's real terminal ([`terminal`]).
//! Epic 3 story 1 adds `agentic run <workflow>` and `agentic status`
//! ([`run`]): a configured workflow's run on am-engine's driver. Story 2.2
//! wires the egress client ([`egress`]): the project's egress directory and
//! the store-backed audit sink; the spike's `c1` and `c4` commands left the
//! binary (owner decision 9, 2026-11-22). Story 2.12 moves Accounts to this
//! Mac ([`user`]) and makes [`key_use`] the one check before any key read.
//! Story 3.2 adds [`serving`]: `agentic serve`'s owner-only check at start
//! and its per-launch CLI token file. Story 3.4 adds `agentic gate`
//! ([`gate`]): the owner decides open Inbox items on a terminal. Story 3.7
//! prices a run's steps from its pinned snapshot (`prices`) and adds spend
//! to `agentic status` (`spend`).
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
pub mod egress;
pub mod gate;
pub mod identity;
pub mod key_use;
mod prices;
pub mod run;
pub mod runtime;
pub mod serving;
mod spend;
pub mod spike;
pub mod terminal;
pub mod user;

use std::sync::Arc;

use am_domain::SecretStore;

/// The Anthropic model `agentic accounts test` calls when none is named.
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-opus-5-5";

/// The OS keychain store.
pub fn keychain() -> Arc<dyn SecretStore> {
    Arc::new(am_secrets::KeychainStore::new())
}
