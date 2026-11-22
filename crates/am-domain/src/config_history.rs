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

use std::collections::BTreeMap;

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
    /// A change made outside the app (another editor, git) and applied by a
    /// Reload (story 1.9); names no earlier entry. `before` or `after` is
    /// empty for a file created or deleted outside.
    OutsideEdit,
    /// A config file stepped to the current `schema_version` on the owner's
    /// accept of a migration offer (story 1.10, AD-7); names no earlier
    /// entry.
    Migration,
    /// A new config file written by a `ConfigCreate` (story 2.1, AD-16):
    /// `before` is empty and it names no earlier entry. The writer never
    /// deletes a file, so a create has no Undo.
    Create,
    /// The one file removal the writer makes (story 2.13, AD-15 *Migrate*,
    /// AD-16): a project's old `models/accounts.yaml`, removed as a step of
    /// a `ConfigMigration`. `before` is the removed bytes, `after` empty,
    /// and it names no earlier entry.
    Removal,
}

impl HistoryKind {
    /// The stored name: `edit`, `undo`, `restore`, `outside_edit`,
    /// `migration`, `create` or `removal`.
    pub fn as_str(self) -> &'static str {
        match self {
            HistoryKind::Edit => "edit",
            HistoryKind::Undo => "undo",
            HistoryKind::Restore => "restore",
            HistoryKind::OutsideEdit => "outside_edit",
            HistoryKind::Migration => "migration",
            HistoryKind::Create => "create",
            HistoryKind::Removal => "removal",
        }
    }

    pub fn parse(text: &str) -> Option<HistoryKind> {
        match text {
            "edit" => Some(HistoryKind::Edit),
            "undo" => Some(HistoryKind::Undo),
            "restore" => Some(HistoryKind::Restore),
            "outside_edit" => Some(HistoryKind::OutsideEdit),
            "migration" => Some(HistoryKind::Migration),
            "create" => Some(HistoryKind::Create),
            "removal" => Some(HistoryKind::Removal),
            _ => None,
        }
    }
}

/// An extra audit entry carried by a config write (AD-15, AD-25): written
/// in the transaction that commits (or reconciles) the write's History
/// entry, right after its `config.*` entry, with the intent's own Actor
/// (who asked for the write), also when the system reconciles it. It
/// holds ids, hosts, ports and paths in plain words; never file content and
/// never a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditNote {
    /// Dotted action, e.g. `config.widened`. Never empty.
    pub action: String,
    /// What was acted on (an id). Never empty.
    pub target: String,
    /// The entry's redacted args, e.g. `kind: account_added`, `host`.
    pub args: BTreeMap<String, String>,
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
    /// Audit entries written with the commit (or reconcile) of this intent,
    /// and only then; empty for an ordinary write.
    pub audit: Vec<AuditNote>,
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
    /// entry `config.edited`, `config.undone`, `config.restored`,
    /// `config.outside_edit`, `config.migrated` or `config.created`, then
    /// the intent's [`AuditNote`]s) in one transaction.
    fn commit_entry(&self, actor: Actor, intent_id: String) -> Result<HistoryEntry, HistoryError>;
    /// Removes an intent whose file was never written (`config.intent_dropped`).
    fn drop_intent(&self, actor: Actor, intent_id: String) -> Result<(), HistoryError>;
    /// Completes an intent whose file already holds its new bytes
    /// (`config.reconciled`, then the intent's [`AuditNote`]s); the entry
    /// keeps the intent's Actor.
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
