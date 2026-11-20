//! The `ConfigHistory` port (AD-16, R3): where the config writer records each
//! step of a `ConfigEdit`, Undo or Restore, in AD-16's apply order:
//!
//! 1. [`ConfigHistory::begin_intent`] commits an intent row (file, old and
//!    new sha256, both byte strings and the diff) before the file is touched;
//! 2. the writer writes and renames the file;
//! 3. [`ConfigHistory::commit_entry`] turns the intent into a History entry,
//!    with its audit entry, in one transaction.
//!
//! At startup [`ConfigHistory::pending_intents`] lists what a crash left:
//! [`ConfigHistory::reconcile_intent`] completes one whose file already has
//! the new hash, [`ConfigHistory::drop_intent`] removes one whose file does
//! not. Every call names its [`Actor`]; there is no default. The port is
//! synchronous and owned-typed; am-store implements it, am-testkit fakes it.

use serde::{Deserialize, Serialize};

use crate::Actor;

/// What a History entry did to its file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryKind {
    /// A `ConfigEdit` through the command path.
    Edit,
    /// Wrote an earlier entry's before-bytes back (`ref_entry` names it).
    Undo,
    /// Wrote an older entry's after-bytes back (`ref_entry` names it).
    Restore,
}

impl HistoryKind {
    /// The stored name: `edit`, `undo` or `restore`.
    pub fn as_str(self) -> &'static str {
        match self {
            HistoryKind::Edit => "edit",
            HistoryKind::Undo => "undo",
            HistoryKind::Restore => "restore",
        }
    }

    pub fn parse(text: &str) -> Option<HistoryKind> {
        match text {
            "edit" => Some(HistoryKind::Edit),
            "undo" => Some(HistoryKind::Undo),
            "restore" => Some(HistoryKind::Restore),
            _ => None,
        }
    }
}

/// One write the config writer is about to make. Config files hold no
/// secrets (the validator refuses them), so the bytes may be stored (AD-35).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIntent {
    /// Project-relative path with forward slashes, e.g. `seats/alpha.md`.
    pub file: String,
    pub kind: HistoryKind,
    /// The entry an Undo or Restore acts on.
    pub ref_entry: Option<String>,
    /// Lowercase hex sha256 of `before` and `after`.
    pub before_sha256: String,
    pub after_sha256: String,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
    /// Unified line diff from `before` to `after`.
    pub diff: String,
}

/// An intent committed but not yet completed or dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingIntent {
    /// The id the History entry takes when the intent completes (UUIDv7).
    pub id: String,
    /// UTC RFC 3339 with ms and `Z`.
    pub at: String,
    /// Who asked for the write.
    pub actor: Actor,
    pub intent: NewIntent,
}

/// One History entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// UUIDv7 text (the intent's id).
    pub id: String,
    /// Order of entries, increasing.
    pub seq: i64,
    /// UTC RFC 3339 with ms and `Z`, when the entry committed.
    pub at: String,
    /// Who made the change (with the hat, for a person).
    pub actor: Actor,
    pub file: String,
    pub kind: HistoryKind,
    pub ref_entry: Option<String>,
    pub before_sha256: String,
    pub after_sha256: String,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
    pub diff: String,
}

impl HistoryEntry {
    /// Who made the change, in History's words, e.g. "You (Reviewer)".
    pub fn who(&self) -> String {
        self.actor.describe()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HistoryError {
    #[error("no pending config intent {0}")]
    NoIntent(String),
    #[error("History could not be recorded: {0}")]
    Store(String),
}

/// Where config History lives (am-store in the app).
pub trait ConfigHistory: Send + Sync {
    /// Commits the intent row (with its audit entry); returns its id.
    fn begin_intent(&self, actor: Actor, intent: NewIntent) -> Result<String, HistoryError>;
    /// Deletes the intent and inserts its History entry (with the audit
    /// entry `config.edited`, `config.undone` or `config.restored`) in one
    /// transaction.
    fn commit_entry(&self, actor: Actor, intent_id: String) -> Result<HistoryEntry, HistoryError>;
    /// Removes an intent whose file was never written (`config.intent_dropped`).
    fn drop_intent(&self, actor: Actor, intent_id: String) -> Result<(), HistoryError>;
    /// Completes an intent whose file already holds its new bytes
    /// (`config.reconciled`); the entry keeps the intent's Actor.
    fn reconcile_intent(
        &self,
        actor: Actor,
        intent_id: String,
    ) -> Result<HistoryEntry, HistoryError>;
    fn pending_intents(&self) -> Result<Vec<PendingIntent>, HistoryError>;
    /// Every entry in `seq` order, or only those of one file.
    fn entries(&self, file: Option<String>) -> Result<Vec<HistoryEntry>, HistoryError>;
    fn entry(&self, id: String) -> Result<Option<HistoryEntry>, HistoryError>;
}
