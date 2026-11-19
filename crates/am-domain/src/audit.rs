//! Audit port. The store's writer is the real sink (AD-25); tools and adapters
//! report refusals and accesses through it.

use crate::Actor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub actor: Actor,
    /// Dotted action, e.g. `folder.read`, `folder.refused`, `egress.blocked`.
    pub action: String,
    /// What was acted on (ids, root and relative path; never row content or secrets).
    pub target: String,
    /// `ok` or a short refusal reason.
    pub outcome: String,
    /// Content hash when relevant (sha256 hex).
    pub hash: Option<String>,
}

pub trait AuditSink: Send + Sync {
    fn record(&self, event: AuditEvent);
}
