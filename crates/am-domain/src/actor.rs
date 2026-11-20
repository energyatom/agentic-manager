//! `Actor`: who performed a mutation (AD-24). Every writer command takes one,
//! with no default.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `Actor` deliberately has no `Default` (and must never get one): every
/// write names who made it, so `am_store::Writer::exec(actor, cmd)` cannot be
/// called without one. am-store's `compile_fail` doctests on `Writer::exec`
/// prove both (story 1.6, run by `just check`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actor {
    /// A person (the owner in v1) wearing a hat, e.g. "Reviewer".
    Human { person: String, hat: String },
    /// A seat acting inside a run step.
    Seat {
        seat: String,
        run: Uuid,
        step: String,
    },
    /// The Assistant, acting through a proposal.
    Assistant { proposal: Uuid },
    /// An import from a named source.
    Import { source: String },
    /// A component of the app itself (scheduler, resume, migrations).
    System { component: String },
}

impl Actor {
    /// The words used in History, provenance and audit, e.g. "You (Reviewer)".
    pub fn describe(&self) -> String {
        match self {
            Actor::Human { hat, .. } => format!("You ({hat})"),
            Actor::Seat { seat, step, .. } => format!("Seat {seat} (step {step})"),
            Actor::Assistant { proposal } => format!("Assistant (proposal {proposal})"),
            Actor::Import { source } => format!("Import ({source})"),
            Actor::System { component } => format!("System ({component})"),
        }
    }
}
