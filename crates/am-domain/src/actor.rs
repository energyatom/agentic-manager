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

/// The hat the owner wears when they resolve an Inbox item (AD-24's product
/// word, story 3.4): every resolution reads "You (Reviewer)". It is not a
/// role template.
pub const REVIEWER_HAT: &str = "Reviewer";

/// A person, and only a person (AD-15, AD-28; story 3.4): the one actor
/// that resolves an Inbox item. Its fields are private, it is built only
/// from a person and a hat with [`HumanActor::new`], and it becomes an
/// [`Actor`] one way only (`From<HumanActor> for Actor`): there is no
/// `From<Actor>`, no `Default` and no `Deserialize`, so no seat, tool,
/// engine, import or Assistant value can pass for one. Only the two command
/// handlers build one (`cargo xtask check-deps`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanActor {
    person: String,
    hat: String,
}

impl HumanActor {
    /// The person `person` wearing `hat` (the owner as [`REVIEWER_HAT`]).
    pub fn new(person: impl Into<String>, hat: impl Into<String>) -> HumanActor {
        HumanActor {
            person: person.into(),
            hat: hat.into(),
        }
    }
}

impl From<HumanActor> for Actor {
    fn from(h: HumanActor) -> Actor {
        Actor::Human {
            person: h.person,
            hat: h.hat,
        }
    }
}
