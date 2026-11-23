//! The config writer (AD-16, R3, story 1.8): the only code that writes
//! project config files.
//!
//! A [`ConfigEdit`] names one existing config file by its project-relative
//! path and a list of [`EditOp`]s on its parsed tree (and, for a seat or a
//! skill, an optional new Markdown body; without one the body is kept byte
//! for byte). The writer emits the result canonically (see `canon`), checks
//! that the change adds no error the project did not already have, and then
//! follows
//! AD-16's apply order through the [`ConfigHistory`] port:
//!
//! 1. commit an intent row (file, old and new sha256, both byte strings, the
//!    diff);
//! 2. write `.<name>.am-tmp` beside the file, keep the file's mode, fsync,
//!    rename over the file, fsync the folder;
//! 3. commit the History entry with its audit entry.
//!
//! [`ConfigWriter::undo`] writes an entry's before-bytes back, refused when
//! the file changed since that entry; [`ConfigWriter::restore`] writes an
//! older entry's after-bytes back. Both are new History entries naming the
//! entry they act on. Every write is refused while the file holds a
//! secret-looking value, so no key reaches History (the owner removes it by
//! hand), and a path must name each folder and file by its exact spelling. [`ConfigWriter::reconcile`] runs at startup: an intent
//! whose file holds its new bytes is completed, any other is dropped; stale
//! temp files are removed when a writer opens. One edit runs at a time. The
//! writer remembers the (file, sha256) of its own writes for the watcher.
//! It never deletes or renames a config file (the one removal is a
//! migration's, below), and it makes a new one only through a
//! [`ConfigCreate`].
//!
//! [`ConfigWriter::create`] (a [`ConfigCreate`], story 2.1, AD-16) writes a
//! new config file through the same intent and commit, under
//! `HistoryKind::Create`, placed without replacing anything: the temp file
//! is fsynced, hard-linked to the final name (`std::fs::hard_link` fails
//! when the name exists, so a file that appeared meanwhile refuses the
//! create and stays as it is), removed, and the folder is fsynced. Missing
//! parent folders inside the project are created. A volume that refuses
//! hard links refuses the create in words, and Undo of a create is refused
//! in words, because the writer never deletes a file. A write may carry
//! [`AuditNote`]s ([`ConfigWriter::apply_with_audit`], `ConfigCreate.audit`):
//! the History port writes them with the commit (AD-15, AD-25).
//!
//! [`ConfigWriter::assign_identity`] (a [`ConfigIdentityAssign`], story 1.13,
//! AD-32) is the one write that does not emit canonically: it inserts a
//! single `project_id: <id>` line directly after `schema_version` and leaves
//! every other byte of `company.yaml` as it was (comments, key order, blank
//! lines and line endings), through the same intent, rename and commit, so
//! `reconcile` settles a crash after the rename. It commits History and its
//! audit only, never an approved grant set (AD-16).
//!
//! Story 2.13 (AD-15 *Migrate*, AD-16): [`ConfigWriter::apply_migration`]
//! applies a [`ConfigMigration`]: its Account renames (one `migration`
//! entry per file) and then the removal of the old `models/accounts.yaml`,
//! the one file the writer ever removes, as a `removal` entry whose intent
//! is committed before the unlink (reconcile treats an absent file as
//! written). [`ConfigWriter::restore_legacy_accounts`] puts that file back
//! byte for byte from a backup (`accounts migrate --rollback`), as a
//! `create`. Both are for the owner only, and only in a project.
//!
//! Story 1.10 (AD-7): the owner's accept of migration offers (see `migrate`)
//! writes every offered file as one `migration` entry each through the same
//! sequence, after one check of the whole migrated project; it is refused
//! before any write when a file changed since it was offered.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use am_domain::{
    Actor, AuditNote, ConfigHistory, HistoryEntry, HistoryError, HistoryKind, NewIntent,
    PendingIntent,
};
use sha2::{Digest, Sha256};

use crate::account_migration::{ConfigMigration, MigrationStep, plan_renames};
use crate::canon::{self, FileKind};
use crate::finding::Report;
use crate::migrate::{self, MigrationOffer};
use crate::user_accounts::{LEGACY_ACCOUNTS_FILE, USER_ACCOUNTS_FILE};
use crate::{
    FOLDERS, Finding, LoadError, LoadOptions, MODEL_FILES, Severity, checks, linediff, load_exact,
    load_with,
};

/// One change to a file's tree. A path names mapping keys, and list items by
/// their number from 0 (`["teams", "1", "name"]`).
#[derive(Debug, Clone, PartialEq)]
pub enum EditOp {
    /// Sets the value at `path`, creating missing mappings on the way; a list
    /// item one past the end is appended.
    Set {
        path: Vec<String>,
        value: serde_json::Value,
    },
    /// Removes the key or list item at `path`.
    Remove { path: Vec<String> },
}

/// One edit of one existing config file. Epic 4's typed patch compiles down
/// to these ops.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigEdit {
    /// Project-relative path with forward slashes, e.g. `seats/alpha.md`.
    pub file: String,
    pub ops: Vec<EditOp>,
    /// A seat's or skill's new Markdown body; `None` keeps it byte for byte.
    pub body: Option<String>,
}

/// One new config file (story 2.1, AD-16): placed only when nothing stands
/// at its name, never over a file. It carries the same intent, History and
/// audit as a [`ConfigEdit`], as a `create` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigCreate {
    /// Project-relative path with forward slashes, e.g. `models/accounts.yaml`.
    pub file: String,
    /// The file's content; it is written canonically.
    pub bytes: Vec<u8>,
    /// Audit entries written with the commit of the create (AD-15, AD-25).
    pub audit: Vec<AuditNote>,
}

/// The System mint of a missing `project_id` (AD-32): one line inserted in
/// `company.yaml` after `schema_version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigIdentityAssign {
    /// The project id (UUIDv7, lowercase hyphenated).
    pub project_id: String,
}

/// Where a test stops a write as a crash would (`test-support` only).
#[cfg(feature = "test-support")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailPoint {
    /// After the intent and the temp file, before the rename (for a
    /// create: before the file is placed).
    BeforeRename,
    /// After the rename (for a create: after the file is placed), before
    /// History is committed.
    AfterRename,
    /// A create's hard link is refused, as on a volume without hard links.
    NoHardLinks,
}

/// Why a write was refused or did not finish.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("refused: {file}: {reason}")]
    BadPath { file: String, reason: String },
    /// Story 3.3, owner condition 3 (AD-15): an agent (a seat inside a
    /// step, or the Assistant) has no config path; only the owner's own
    /// commands write config, so a cap can never be raised by an agent.
    #[error(
        "refused: {actor} cannot change the project's config: only your own commands write config files (AD-15), and nothing was written"
    )]
    AgentWrite { actor: String },
    #[error("refused: {file} cannot be edited because {reason}; fix the file first")]
    NotEditable { file: String, reason: String },
    #[error("refused: {0}")]
    BadEdit(String),
    #[error("refused: the change would leave the project invalid:\n{}", Lines(.findings))]
    Invalid {
        file: String,
        findings: Vec<Finding>,
    },
    #[error("refused: {0}")]
    NoChange(String),
    /// Story 1.9: the session refuses the app's own write while a config
    /// file on disk differs from the accepted bytes (an outside change not
    /// yet reloaded), so the write never takes that change in silently. The
    /// writer gives the same refusal when a file changed on disk after a
    /// write (edit, Undo, Restore or identity) read it and before it is
    /// written; a migration accept reports it as `ChangedSince`.
    #[error(
        "refused: {} changed on disk outside the app; Reload (or undo that change) first, then try again",
        .files.join(", ")
    )]
    ChangedOnDisk { files: Vec<String> },
    /// The file no longer holds the bytes an Undo's entry wrote, or (story
    /// 1.10, `entry` empty) the bytes it held when a migration was offered.
    #[error("{}", changed_since(.file, .entry))]
    ChangedSince { file: String, entry: String },
    /// Story 1.10 (AD-7): a migration is accepted only by the owner.
    #[error(
        "refused: only the owner accepts a config migration (AD-7); {actor} cannot, and nothing was written"
    )]
    NotOwner { actor: String },
    /// Story 1.10: a migration accept stopped part-way (a file changed, an
    /// I/O or History error). `migrated` were written, each with its History
    /// entry; `file` was written too when `written` (a failure after its
    /// rename: its entry is recorded by `reconcile` at the next start); the
    /// rest were not.
    #[error("{}", stopped_words(.file, .migrated, *.written, .cause))]
    MigrationStopped {
        file: String,
        migrated: Vec<String>,
        written: bool,
        cause: Box<WriteError>,
    },
    /// Story 1.10: an empty accept while no config file is older.
    #[error(
        "refused: there is nothing to migrate; every config file is at the schema_version this app reads"
    )]
    NothingToMigrate {},
    #[error(
        "refused: {file} holds a secret-looking value (line {}); remove it by hand first, then make the change (keys live in the Keychain; reference an Account instead), so that no key is ever kept in History",
        Joined(.lines)
    )]
    Secret { file: String, lines: Vec<u32> },
    #[error("refused: there is no History entry {0}")]
    UnknownEntry(String),
    #[error("the write stopped: {0}")]
    Interrupted(String),
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
    #[error(transparent)]
    History(#[from] HistoryError),
}

/// The words of [`WriteError::MigrationStopped`].
fn stopped_words(file: &str, migrated: &[String], written: bool, cause: &WriteError) -> String {
    let mut all = migrated.to_vec();
    if written {
        all.push(file.to_string());
    }
    let note = match (written, migrated.is_empty()) {
        (true, true) => format!("{file}'s History entry is recorded at the next start"),
        (true, false) => format!(
            "{file}'s History entry is recorded at the next start; each other file has its entry"
        ),
        (false, _) => "each has its History entry".to_string(),
    };
    format!(
        "stopped at {file}: {cause}; already migrated: {} ({note}); ask for the migration offers again for the rest",
        all.join(", ")
    )
}

/// The refusal of an accept that would leave older files behind.
fn left_behind(why: &[String]) -> WriteError {
    WriteError::BadEdit(format!(
        "the accept would leave config files at an older schema_version, so nothing was migrated:\n- {}",
        why.join("\n- ")
    ))
}

/// The words of [`WriteError::ChangedSince`].
fn changed_since(file: &str, entry: &str) -> String {
    if entry.is_empty() {
        format!(
            "refused: {file} changed on disk since the migration was offered, so it was not overwritten; ask for the migration offers again and review the new diff"
        )
    } else {
        format!(
            "refused: {file} changed since this entry ({entry}), so undoing it would lose the later change; restore an entry instead"
        )
    }
}

/// Findings, one per line block.
struct Lines<'f>(&'f [Finding]);

impl fmt::Display for Lines<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, finding) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{finding}")?;
        }
        Ok(())
    }
}

fn io(context: String) -> impl FnOnce(std::io::Error) -> WriteError {
    move |source| WriteError::Io { context, source }
}

/// What [`ConfigWriter::reconcile`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileReport {
    /// Intents whose file held the new bytes, now History entries.
    pub completed: Vec<HistoryEntry>,
    /// Ids of intents whose file was never written.
    pub dropped: Vec<String>,
}

/// Line numbers, comma-separated.
struct Joined<'l>(&'l [u32]);

impl fmt::Display for Joined<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let words: Vec<String> = self.0.iter().map(u32::to_string).collect();
        f.write_str(&words.join(", "))
    }
}

/// The lines of `bytes` (the config file `file` as it is now) that hold a
/// secret-looking value, as the validator detects one: a key-shaped text on
/// any line, or a secret-named key holding a value.
pub(crate) fn secret_lines(file: &str, bytes: &[u8]) -> Vec<u32> {
    secret_lines_of(canon::kind_of(file), file, bytes)
}

/// [`secret_lines`] for a file of `kind`.
fn secret_lines_of(kind: Option<FileKind>, file: &str, bytes: &[u8]) -> Vec<u32> {
    let text = String::from_utf8_lossy(bytes);
    let mut rep = Report::new(file);
    checks::text_secrets(&mut rep, &text, 1);
    if let Some(kind) = kind
        && let Ok(doc) = canon::parse(kind, &text)
    {
        // A Markdown file's frontmatter starts on line 2.
        let offset = u32::from(kind.markdown());
        let mut tree_rep = Report::new(file);
        checks::secrets(&mut tree_rep, &doc.tree);
        rep.findings
            .extend(tree_rep.findings.into_iter().map(|mut f| {
                f.line += offset;
                f
            }));
    }
    let mut lines: Vec<u32> = rep.findings.iter().map(|f| f.line).collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// How many of its own writes the writer remembers for the watcher.
const OWN_WRITES: usize = 1024;
const TMP_SUFFIX: &str = ".am-tmp";

/// The sha256 of `bytes` as 64 lowercase hex characters: the hash the
/// writer, History and the `accounts migrate` backup compare.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The project's config writer. Build one per open project; or, with
/// [`ConfigWriter::for_user_accounts`], the writer of this Mac's user
/// Accounts file (story 2.12).
pub struct ConfigWriter {
    root: PathBuf,
    /// The writer of the user Accounts file: `root` is the app data dir and
    /// `accounts.yaml` the one file it writes (AD-17).
    user: bool,
    history: Arc<dyn ConfigHistory>,
    /// Runs each intent, write and commit sequence alone.
    serial: Mutex<()>,
    own: Mutex<VecDeque<(String, String)>>,
    #[cfg(feature = "test-support")]
    fail: Mutex<Option<FailPoint>>,
}

impl ConfigWriter {
    /// A writer for the project at `root`; removes temp files a crashed
    /// write left in the config folders.
    pub fn open(root: &Path, history: Arc<dyn ConfigHistory>) -> Result<ConfigWriter, WriteError> {
        if !root.is_dir() {
            return Err(WriteError::BadPath {
                file: root.display().to_string(),
                reason: "there is no project folder here".into(),
            });
        }
        for dir in config_dirs(root) {
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in read.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                // `file_type` does not follow links: a temp-named link goes too.
                let removable = e.file_type().is_ok_and(|t| t.is_file() || t.is_symlink());
                if removable && name.starts_with('.') && name.ends_with(TMP_SUFFIX) {
                    std::fs::remove_file(e.path())
                        .map_err(io(format!("cannot remove {}", e.path().display())))?;
                }
            }
        }
        Ok(ConfigWriter {
            root: root.to_path_buf(),
            user: false,
            history,
            serial: Mutex::new(()),
            own: Mutex::new(VecDeque::new()),
            #[cfg(feature = "test-support")]
            fail: Mutex::new(None),
        })
    }

    /// The writer of this Mac's user Accounts file (story 2.12; AD-16,
    /// AD-17): `app_data` is the app data dir, and `accounts.yaml` in it is
    /// the one file it writes, with the same atomic write, intent, History,
    /// self-write fingerprint and `ConfigCreate` as a project file; any other
    /// name is refused. `history` is the user store's (its guard holds
    /// `user.lock`). Removes a temp file a crashed write left. The file is
    /// written owner-only (0600).
    pub fn for_user_accounts(
        app_data: &Path,
        history: Arc<dyn ConfigHistory>,
    ) -> Result<ConfigWriter, WriteError> {
        if !app_data.is_dir() {
            return Err(WriteError::BadPath {
                file: app_data.display().to_string(),
                reason: "there is no app data folder here".into(),
            });
        }
        let tmp = app_data.join(format!(".{USER_ACCOUNTS_FILE}{TMP_SUFFIX}"));
        if std::fs::symlink_metadata(&tmp).is_ok_and(|m| m.is_file() || m.file_type().is_symlink())
        {
            std::fs::remove_file(&tmp).map_err(io(format!("cannot remove {}", tmp.display())))?;
        }
        Ok(ConfigWriter {
            root: app_data.to_path_buf(),
            user: true,
            history,
            serial: Mutex::new(()),
            own: Mutex::new(VecDeque::new()),
            #[cfg(feature = "test-support")]
            fail: Mutex::new(None),
        })
    }

    /// The kind of `file` this writer writes: the user Accounts file only
    /// (user mode), or a project's config file.
    fn kind_of(&self, file: &str) -> Option<FileKind> {
        if self.user {
            (file == USER_ACCOUNTS_FILE).then_some(FileKind::Accounts)
        } else {
            canon::kind_of(file)
        }
    }

    /// The refusal of a name this writer does not write.
    fn not_written(&self) -> &'static str {
        if self.user {
            "not the file this writer writes: it writes only this Mac's Accounts file, accounts.yaml"
        } else {
            "not a config file the app writes (company.yaml, tools.yaml, models/, seats/, \
             skills/<id>/SKILL.md, workflows/, tables/, dashboards/, sources/ or schemas/)"
        }
    }

    /// Stops the next writes at `point` as a crash would (tests only).
    #[cfg(feature = "test-support")]
    pub fn fail_at(&self, point: Option<FailPoint>) {
        if let Ok(mut f) = self.fail.lock() {
            *f = point;
        }
    }

    /// Story 3.3, owner condition 3 (AD-15): a write whose actor is a seat
    /// or the Assistant is refused first, before any read, intent or write.
    fn refuse_agent(actor: &Actor) -> Result<(), WriteError> {
        match actor {
            Actor::Seat { .. } | Actor::Assistant { .. } => Err(WriteError::AgentWrite {
                actor: actor.describe(),
            }),
            Actor::Human { .. } | Actor::Import { .. } | Actor::System { .. } => Ok(()),
        }
    }

    fn serial(&self) -> Result<std::sync::MutexGuard<'_, ()>, WriteError> {
        self.serial
            .lock()
            .map_err(|_| WriteError::Interrupted("an earlier write panicked".into()))
    }

    /// Applies `edit` by `actor` and returns its History entry.
    pub fn apply(&self, actor: Actor, edit: ConfigEdit) -> Result<HistoryEntry, WriteError> {
        self.apply_checked(actor, edit, None, Vec::new())
    }

    /// [`ConfigWriter::apply`] for an edit built from an earlier read of the
    /// file (story 2.1, the interim CLI Account edit): it is applied only
    /// while the file still holds the bytes it was built on, whose sha256
    /// is `before_sha256`, and refused as `ChangedOnDisk` with nothing
    /// written otherwise (its ops name list items by number, so on other
    /// bytes they would change another item). `audit` are entries the
    /// History port writes in the transaction that commits the edit
    /// (AD-15's widening record of an Account added from the CLI).
    pub fn apply_with_audit(
        &self,
        actor: Actor,
        edit: ConfigEdit,
        before_sha256: &str,
        audit: Vec<AuditNote>,
    ) -> Result<HistoryEntry, WriteError> {
        self.apply_checked(actor, edit, Some(before_sha256), audit)
    }

    /// Story 2.14 (review follow-up): every check [`ConfigWriter::apply_with_audit`]
    /// runs on `edit` (the bytes it was built on, a secret-looking value in
    /// the file now or after the edit, an error the edit adds), writing and
    /// recording nothing; a caller runs it before an effect it cannot take
    /// back.
    pub fn check_edit(&self, edit: ConfigEdit, before_sha256: &str) -> Result<(), WriteError> {
        let _one = self.serial()?;
        let (file, before, after) = self.edited(edit, Some(before_sha256))?;
        let lines = secret_lines_of(self.kind_of(&file), &file, &after);
        if !lines.is_empty() {
            return Err(WriteError::Secret { file, lines });
        }
        self.step_checks(&file, &before, &after)
    }

    fn apply_checked(
        &self,
        actor: Actor,
        edit: ConfigEdit,
        before_sha256: Option<&str>,
        audit: Vec<AuditNote>,
    ) -> Result<HistoryEntry, WriteError> {
        Self::refuse_agent(&actor)?;
        let _one = self.serial()?;
        let (file, before, after) = self.edited(edit, before_sha256)?;
        self.step(
            actor,
            &file,
            Change {
                kind: HistoryKind::Edit,
                ref_entry: None,
                before,
                after,
                audit,
            },
        )
    }

    /// The file `edit` names, its bytes now and its bytes after `edit`.
    fn edited(
        &self,
        edit: ConfigEdit,
        before_sha256: Option<&str>,
    ) -> Result<(String, Vec<u8>, Vec<u8>), WriteError> {
        let kind = self.resolve(&edit.file)?;
        let file = edit.file;
        let before = self.read(&file)?;
        if before_sha256.is_some_and(|expected| sha256_hex(&before) != expected) {
            return Err(WriteError::ChangedOnDisk { files: vec![file] });
        }
        let text = String::from_utf8(before.clone()).map_err(|_| WriteError::NotEditable {
            file: file.clone(),
            reason: "it is not UTF-8 text".into(),
        })?;
        let mut doc = canon::parse(kind, &text).map_err(|reason| WriteError::NotEditable {
            file: file.clone(),
            reason,
        })?;
        for op in &edit.ops {
            let (path, value) = match op {
                EditOp::Set { path, value } => (path, Some(value)),
                EditOp::Remove { path } => (path, None),
            };
            canon::apply(&mut doc.tree, path, value)
                .map_err(|e| WriteError::BadEdit(format!("{file}: {e}")))?;
        }
        if let Some(body) = edit.body {
            if !kind.markdown() {
                return Err(WriteError::BadEdit(format!(
                    "{file} has no Markdown body; only seat files and SKILL.md have one"
                )));
            }
            doc.body = Some(body);
        }
        let after = canon::emit(&doc)
            .map_err(|e| WriteError::BadEdit(format!("{file}: {e}")))?
            .into_bytes();
        if after == before {
            return Err(WriteError::NoChange(format!(
                "the edit changes nothing in {file}"
            )));
        }
        Ok((file, before, after))
    }

    /// Creates the config file `create.file` holding `create.bytes` (written
    /// canonically) and returns its `create` History entry. Refused, with
    /// nothing written, when anything already stands at that name (also one
    /// that appears while the create runs: it stays as it is), when the path
    /// is not a config file inside the project or a parent is a link, when
    /// the content holds a secret-looking value, when the project with the
    /// new file has an error it does not have now, and when the volume
    /// refuses hard links. Missing parent folders are created.
    pub fn create(&self, actor: Actor, create: ConfigCreate) -> Result<HistoryEntry, WriteError> {
        Self::refuse_agent(&actor)?;
        let _one = self.serial()?;
        let file = create.file;
        let kind = self.plain_config_path(&file)?;
        // An intent of this file left pending is settled first, as at the
        // next start: a create interrupted after placing is then an entry,
        // and this create finds its file.
        for p in self.history.pending_intents()? {
            if p.intent.file == file {
                self.settle(p)?;
            }
        }
        self.resolve_absent(&file)?;
        // No key may reach a config file, an intent or a diff.
        let secret = |bytes: &[u8]| {
            let lines = secret_lines_of(Some(kind), &file, bytes);
            if lines.is_empty() {
                return Ok(());
            }
            Err(WriteError::BadEdit(format!(
                "the new {file} would hold a secret-looking value (line {}); keys live in the Keychain: reference an Account's key with `secret_ref` instead. Nothing was written",
                Joined(&lines)
            )))
        };
        secret(&create.bytes)?;
        let text = String::from_utf8(create.bytes).map_err(|_| WriteError::NotEditable {
            file: file.clone(),
            reason: "the new content is not UTF-8 text".into(),
        })?;
        let doc = canon::parse(kind, &text).map_err(|reason| WriteError::NotEditable {
            file: file.clone(),
            reason,
        })?;
        let after = canon::emit(&doc)
            .map_err(|e| WriteError::BadEdit(format!("{file}: {e}")))?
            .into_bytes();
        secret(&after)?;
        if after.is_empty() {
            return Err(WriteError::NoChange(format!(
                "the new {file} would be empty"
            )));
        }
        // As for an edit: refused only when the new file adds an error. The
        // project is read from an exact map of its tracked files, with and
        // without the new one (a missing file cannot be overridden).
        let now = self.scan();
        let mut with = now.clone();
        with.insert(file.clone(), after.clone());
        let new_errors = added_errors(self.errors_exact(now)?, self.errors_exact(with)?);
        if !new_errors.is_empty() {
            return Err(WriteError::Invalid {
                file,
                findings: new_errors,
            });
        }
        let after_sha256 = sha256_hex(&after);
        let intent = NewIntent {
            file: file.clone(),
            kind: HistoryKind::Create,
            ref_entry: None,
            before_sha256: sha256_hex(b""),
            after_sha256: after_sha256.clone(),
            diff: linediff::unified(&file, b"", &after),
            before: Vec::new(),
            after: after.clone(),
            audit: create.audit,
        };
        let id = self.history.begin_intent(actor.clone(), intent)?;
        // Remembered before the file is placed, so the watcher never sees
        // it as an outside change.
        self.remember(&file, &after_sha256);
        match self.place(&file, &after) {
            Ok(()) => {}
            Err(Written::No(e)) => {
                // Nothing of ours stands at the name: the intent goes (best
                // effort; a left intent is dropped by the next reconcile).
                let _ = self.history.drop_intent(actor, id);
                self.forget_own_writes(&file);
                return Err(e);
            }
            Err(Written::Maybe(e)) => return Err(e),
        }
        Ok(self.history.commit_entry(actor, id)?)
    }

    /// Inserts `project_id: <id>` directly after `company.yaml`'s
    /// `schema_version` line (with that line's ending) and returns the
    /// History entry; every other byte stays. Refused when the id is not a
    /// UUIDv7, when the file already holds a `project_id` or has no
    /// `schema_version` line.
    pub fn assign_identity(
        &self,
        actor: Actor,
        assign: ConfigIdentityAssign,
    ) -> Result<HistoryEntry, WriteError> {
        Self::refuse_agent(&actor)?;
        let _one = self.serial()?;
        let id = assign.project_id;
        if am_domain::ProjectId::parse(&id).is_none() {
            return Err(WriteError::BadEdit(format!(
                "'{id}' is not a project id (a lowercase UUIDv7)"
            )));
        }
        let file = "company.yaml";
        self.resolve(file)?;
        let before = self.read(file)?;
        let text = String::from_utf8(before.clone()).map_err(|_| WriteError::NotEditable {
            file: file.to_string(),
            reason: "it is not UTF-8 text".into(),
        })?;
        let doc =
            canon::parse(FileKind::Company, &text).map_err(|reason| WriteError::NotEditable {
                file: file.to_string(),
                reason,
            })?;
        if canon::has_top_key(&doc.tree, "project_id") {
            return Err(WriteError::NoChange(format!(
                "{file} already holds a project_id; the app never writes over one"
            )));
        }
        let after =
            insert_after_schema_version(&text, &format!("project_id: {id}")).ok_or_else(|| {
                WriteError::NotEditable {
                    file: file.to_string(),
                    reason: "it has no `schema_version:` line at the top level".into(),
                }
            })?;
        self.step(
            actor,
            file,
            Change {
                kind: HistoryKind::Edit,
                ref_entry: None,
                before,
                after: after.into_bytes(),
                audit: Vec::new(),
            },
        )
    }

    /// Story 2.14 (AD-15 *Change*, *Every path*; review fix Adversarial 3):
    /// in user mode, refuses an Undo or Restore that would write `target`
    /// over `current` while it adds, removes or moves (another provider or
    /// `base_url`) any Account: "Undo of a user-file change goes through the
    /// same confirming handler", so an Account is added again only by
    /// `agentic accounts add`, removed by `remove` and moved by `change`.
    /// Only a change that leaves every Account where it is (a name) passes.
    fn refuse_account_move(
        &self,
        entry: &HistoryEntry,
        current: &[u8],
        target: &[u8],
        action: &str,
    ) -> Result<(), WriteError> {
        if !self.user {
            return Ok(());
        }
        // The secret check first (review follow-up): no id of a file that
        // holds a secret-looking value is ever named.
        for bytes in [current, target] {
            let lines = secret_lines_of(self.kind_of(&entry.file), &entry.file, bytes);
            if !lines.is_empty() {
                return Err(WriteError::Secret {
                    file: entry.file.clone(),
                    lines,
                });
            }
        }
        let mut shown_ids: BTreeSet<String> = BTreeSet::new();
        let mut destinations = |bytes: &[u8]| -> BTreeMap<String, (String, Option<String>)> {
            let loaded = crate::user_accounts::load_bytes(USER_ACCOUNTS_FILE, bytes);
            let accounts = loaded.accounts.accounts();
            shown_ids.extend(crate::user_accounts::valid_ids(accounts));
            accounts
                .iter()
                .map(|a| (a.id.clone(), (a.provider.clone(), a.base_url.clone())))
                .collect()
        };
        let now = destinations(current);
        let then = destinations(target);
        // Only an id that loaded as a valid Account is named.
        let named = |ids: Vec<&String>| {
            ids.iter()
                .map(|id| {
                    if shown_ids.contains(*id) {
                        format!("'{id}'")
                    } else {
                        "(an entry whose id is not shown)".to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let added: Vec<&String> = then.keys().filter(|id| !now.contains_key(*id)).collect();
        let removed: Vec<&String> = now.keys().filter(|id| !then.contains_key(*id)).collect();
        let moved: Vec<&String> = then
            .iter()
            .filter(|(id, to)| now.get(*id).is_some_and(|from| from != *to))
            .map(|(id, _)| id)
            .collect();
        let mut why = Vec::new();
        if !added.is_empty() {
            why.push(format!(
                "add the Account {} again, which only `agentic accounts add` does (it asks for the typed id and the key)",
                named(added)
            ));
        }
        if !removed.is_empty() {
            why.push(format!(
                "remove the Account {}, which only `agentic accounts remove` does",
                named(removed)
            ));
        }
        if !moved.is_empty() {
            why.push(format!(
                "change the provider or address of the Account {}, which is confirmed only with `agentic accounts change <id>` (a new key when the provider changes)",
                named(moved)
            ));
        }
        if why.is_empty() {
            return Ok(());
        }
        Err(WriteError::BadEdit(format!(
            "{}: the {action} of entry {} would {} on this Mac; nothing was written",
            entry.file,
            entry.id,
            why.join(", and would ")
        )))
    }

    /// Story 2.14 (AD-15 *Record*; review fix G-record): in user mode, a
    /// History entry whose before equals its after, carrying `audit` (a
    /// confirmation of what the file already holds) through the same
    /// intent, write and commit sequence as an edit, so the confirmation
    /// commits in the transaction of its History entry. Applied only while
    /// the file holds the bytes whose sha256 is `before_sha256`; refused in
    /// project mode and with no note.
    pub fn apply_unchanged_with_audit(
        &self,
        actor: Actor,
        before_sha256: &str,
        audit: Vec<AuditNote>,
    ) -> Result<HistoryEntry, WriteError> {
        Self::refuse_agent(&actor)?;
        if !self.user {
            return Err(WriteError::BadEdit(
                "only the writer of this Mac's Accounts file records a confirmation of an unchanged file".into(),
            ));
        }
        if audit.is_empty() {
            return Err(WriteError::NoChange(format!(
                "nothing to record for {USER_ACCOUNTS_FILE}"
            )));
        }
        let _one = self.serial()?;
        let file = USER_ACCOUNTS_FILE.to_string();
        self.resolve(&file)?;
        let before = self.read(&file)?;
        if sha256_hex(&before) != before_sha256 {
            return Err(WriteError::ChangedOnDisk { files: vec![file] });
        }
        self.step(
            actor,
            &file,
            Change {
                kind: HistoryKind::Edit,
                ref_entry: None,
                after: before.clone(),
                before,
                audit,
            },
        )
    }

    /// Writes entry `id`'s before-bytes back, as a new `undo` entry. Refused
    /// when the file no longer holds that entry's after-bytes.
    pub fn undo(&self, actor: Actor, id: &str) -> Result<HistoryEntry, WriteError> {
        Self::refuse_agent(&actor)?;
        let _one = self.serial()?;
        let entry = self.entry(id)?;
        refuse_removal(&entry, "undo")?;
        refuse_absent(&entry, true)?;
        if entry.kind == HistoryKind::Create {
            return Err(WriteError::BadEdit(format!(
                "{}: entry {} created the file, and to undo it would delete the file; the app never deletes a config file, so delete it by hand (and remove what names it first)",
                entry.file, entry.id
            )));
        }
        self.resolve(&entry.file)?;
        let current = self.read(&entry.file)?;
        if sha256_hex(&current) != entry.after_sha256 {
            return Err(WriteError::ChangedSince {
                file: entry.file,
                entry: entry.id,
            });
        }
        self.refuse_account_move(&entry, &current, &entry.before, "undo")?;
        self.step(
            actor,
            &entry.file,
            Change {
                kind: HistoryKind::Undo,
                ref_entry: Some(entry.id.clone()),
                before: current,
                after: entry.before,
                audit: Vec::new(),
            },
        )
    }

    /// Writes entry `id`'s after-bytes back, as a new `restore` entry.
    pub fn restore(&self, actor: Actor, id: &str) -> Result<HistoryEntry, WriteError> {
        Self::refuse_agent(&actor)?;
        let _one = self.serial()?;
        let entry = self.entry(id)?;
        refuse_removal(&entry, "restore")?;
        refuse_absent(&entry, false)?;
        self.resolve(&entry.file)?;
        let current = self.read(&entry.file)?;
        if sha256_hex(&current) == entry.after_sha256 {
            return Err(WriteError::NoChange(format!(
                "{} already holds what entry {} wrote",
                entry.file, entry.id
            )));
        }
        self.refuse_account_move(&entry, &current, &entry.after, "restore")?;
        self.step(
            actor,
            &entry.file,
            Change {
                kind: HistoryKind::Restore,
                ref_entry: Some(entry.id.clone()),
                before: current,
                after: entry.after,
                audit: Vec::new(),
            },
        )
    }

    /// Story 1.10 (AD-7): writes every offered file's migrated bytes as one
    /// `migration` entry each, in the offers' order, or none. Only an
    /// `Actor::Human` (the owner) is accepted. Before any write: each file
    /// must still hold the bytes it held when offered and no key, each offer
    /// must be exactly the app's migration of those bytes, the offers must cover every older file
    /// with a step path (offered or not), and the whole migrated project must
    /// add no error (only added errors refuse, as for an edit). Each file
    /// then goes through the intent, write and commit sequence, after its
    /// bytes are checked once more (an edit saved during the accept stops it
    /// with `ChangedSince`); the per-file validity check is not repeated,
    /// because the set was checked as a whole. An empty accept is
    /// `NothingToMigrate` only when no file is older.
    pub(crate) fn migrate(
        &self,
        actor: Actor,
        offers: Vec<MigrationOffer>,
    ) -> Result<Vec<HistoryEntry>, WriteError> {
        if !matches!(actor, Actor::Human { .. }) {
            return Err(WriteError::NotOwner {
                actor: actor.describe(),
            });
        }
        let _one = self.serial()?;
        // The files as the loader reads them: a link it skips is not one.
        let on_disk = migrate::migration_files(&self.root);
        if offers.is_empty() {
            let mut why = migrate::left_behind(&self.root, &on_disk, &BTreeMap::new());
            why.extend(migrate::unmigratable(&self.root, &on_disk));
            if why.is_empty() {
                return Err(WriteError::NothingToMigrate {});
            }
            return Err(left_behind(&why));
        }
        let mut overrides = BTreeMap::new();
        let mut befores = Vec::with_capacity(offers.len());
        for offer in &offers {
            if overrides.contains_key(&offer.file) {
                return Err(WriteError::BadEdit(format!(
                    "{} is offered twice; ask for the migration offers again",
                    offer.file
                )));
            }
            self.resolve(&offer.file)?;
            let before = self.read(&offer.file)?;
            if sha256_hex(&before) != offer.before_sha256 {
                return Err(WriteError::ChangedSince {
                    file: offer.file.clone(),
                    entry: String::new(),
                });
            }
            let lines = secret_lines(&offer.file, &before);
            if !lines.is_empty() {
                return Err(WriteError::Secret {
                    file: offer.file.clone(),
                    lines,
                });
            }
            overrides.insert(offer.file.clone(), offer.after.clone());
            befores.push(before);
        }
        // Story 1.14: a step sees the whole project, so every file the
        // offers were derived from must still hold the bytes it held then
        // (a Source's owner_seat, another seat's capabilities), and every
        // offer is re-derived from the whole project's current bytes (each
        // offered file as just read and checked above).
        let mut project = on_disk.clone();
        for (offer, before) in offers.iter().zip(&befores) {
            project.insert(offer.file.clone(), before.clone());
        }
        for offer in &offers {
            if let Some(file) = migrate::changed_input(&offer.inputs, &project) {
                return Err(WriteError::ChangedSince {
                    file,
                    entry: String::new(),
                });
            }
        }
        migrate::check_offers(&self.root, &project, &offers)?;
        // All or nothing: a project half on the old version cannot load, so
        // every older file with a step path must be in the accept, whatever
        // kept it from being offered.
        let why = migrate::left_behind(&self.root, &on_disk, &overrides);
        if !why.is_empty() {
            return Err(left_behind(&why));
        }
        let errors_now = self.errors(BTreeMap::new())?;
        let errors_after = self.errors(overrides)?;
        let new_errors = added_errors(errors_now, errors_after);
        if let Some(first) = new_errors.first() {
            return Err(WriteError::Invalid {
                file: first.file.clone(),
                findings: new_errors,
            });
        }
        // Once more after the validation, before the first write: an edit
        // made meanwhile refuses the accept with nothing written.
        for (offer, before) in offers.iter().zip(&befores) {
            self.unchanged(&offer.file, before)?;
        }
        // And every other file the derivation read: a change to one of them
        // changes what the steps produce.
        let project_inputs = migrate::derivation_inputs(&project);
        if let Some(file) =
            migrate::changed_input(&project_inputs, &migrate::migration_files(&self.root))
        {
            return Err(WriteError::ChangedSince {
                file,
                entry: String::new(),
            });
        }
        let mut entries: Vec<HistoryEntry> = Vec::with_capacity(offers.len());
        for (offer, before) in offers.into_iter().zip(befores) {
            let stop = |cause: WriteError, written: bool, entries: Vec<HistoryEntry>| {
                // Nothing reached any file: the refusal stands as it is.
                if entries.is_empty() && !written {
                    return cause;
                }
                WriteError::MigrationStopped {
                    file: offer.file.clone(),
                    migrated: entries.into_iter().map(|e| e.file).collect(),
                    written,
                    cause: Box::new(cause),
                }
            };
            // An owner's save while earlier files were written is never
            // overwritten: the accept stops here, before this file's intent.
            if let Err(e) = self.unchanged(&offer.file, &before) {
                return Err(stop(e, false, entries));
            }
            let after_sha256 = sha256_hex(&offer.after);
            match self.write_step(
                actor.clone(),
                &offer.file,
                Change {
                    kind: HistoryKind::Migration,
                    ref_entry: None,
                    before,
                    after: offer.after.clone(),
                    audit: Vec::new(),
                },
            ) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    // A save during the write is a change since the offer.
                    let e = match e {
                        WriteError::ChangedOnDisk { .. } => WriteError::ChangedSince {
                            file: offer.file.clone(),
                            entry: String::new(),
                        },
                        other => other,
                    };
                    // A failure after the rename (the folder sync, a crash
                    // point, the History commit) leaves the file migrated
                    // with its intent pending; `reconcile` records the entry
                    // at the next start.
                    let written = std::fs::read(self.root.join(&offer.file))
                        .is_ok_and(|b| sha256_hex(&b) == after_sha256);
                    return Err(stop(e, written, entries));
                }
            }
        }
        Ok(entries)
    }

    /// Story 2.13 (AD-15 *Migrate*, AD-16): applies `migration`, every
    /// step or none, for the owner only (`NotOwner` otherwise) and only in a
    /// project (the user Accounts file's writer removes nothing). Before any
    /// write: a removal must name exactly `models/accounts.yaml`, which must
    /// still hold the bytes it names and no key; each rename's new id must be
    /// an Account id; and the project with every step applied must add no
    /// error. Then each renamed file is written as one `migration` entry,
    /// and last the old file is removed as one `removal` entry, its intent
    /// committed before the unlink. Returns the entries in that order.
    /// An `Expects` step names the exact set of files the renames may write
    /// and the bytes each must still hold (what the owner was shown; an
    /// empty set included); a file outside it, or any difference, refuses
    /// with nothing written. Every step is checked before any
    /// is written; should a write still stop part-way, a re-run completes it
    /// (a rename whose `old` is no longer named writes nothing). The old file
    /// is resolved again (exact name, no link) right before its unlink.
    pub fn apply_migration(
        &self,
        actor: Actor,
        migration: ConfigMigration,
    ) -> Result<Vec<HistoryEntry>, WriteError> {
        if !matches!(actor, Actor::Human { .. }) {
            return Err(WriteError::NotOwner {
                actor: actor.describe(),
            });
        }
        if self.user {
            return Err(WriteError::BadEdit(
                "the writer of this Mac's Accounts file makes no migration and removes no file; nothing was written".into(),
            ));
        }
        if migration.steps.is_empty() {
            return Err(WriteError::NoChange("the migration has no step".into()));
        }
        let _one = self.serial()?;
        let mut removal: Option<(String, Vec<u8>)> = None;
        let mut renames: Vec<(String, String)> = Vec::new();
        let mut expects: Option<BTreeMap<String, String>> = None;
        for step in migration.steps {
            match step {
                MigrationStep::Remove {
                    file,
                    before_sha256,
                } => {
                    if file != LEGACY_ACCOUNTS_FILE {
                        return Err(WriteError::BadPath {
                            file,
                            reason: format!(
                                "the only file a migration removes is a project's old {LEGACY_ACCOUNTS_FILE}; nothing was written"
                            ),
                        });
                    }
                    if removal.is_some() {
                        return Err(WriteError::BadEdit(format!(
                            "{file} is removed twice in one migration; nothing was written"
                        )));
                    }
                    self.resolve(&file)?;
                    let bytes = self.read(&file)?;
                    if sha256_hex(&bytes) != before_sha256 {
                        return Err(WriteError::ChangedSince {
                            file,
                            entry: String::new(),
                        });
                    }
                    let lines = secret_lines(&file, &bytes);
                    if !lines.is_empty() {
                        return Err(WriteError::Secret { file, lines });
                    }
                    removal = Some((file, bytes));
                }
                MigrationStep::RenameAccount { old, new } => renames.push((old, new)),
                MigrationStep::Expects { files } => {
                    if expects.is_some() {
                        return Err(WriteError::BadEdit(
                            "a migration names its expected files once; nothing was written".into(),
                        ));
                    }
                    expects = Some(files);
                }
            }
        }
        let disk = self.scan();
        let planned = plan_renames(&self.root, &disk, &renames)?;
        // What the owner was shown: every expected file still holds its
        // bytes, and no other file would be written.
        if let Some(holds) = &expects {
            for (file, expected) in holds {
                let now = std::fs::read(self.root.join(file))
                    .map(|b| sha256_hex(&b))
                    .unwrap_or_default();
                if now != *expected {
                    return Err(WriteError::ChangedSince {
                        file: file.clone(),
                        entry: String::new(),
                    });
                }
            }
            if let Some(r) = planned.iter().find(|r| !holds.contains_key(&r.file)) {
                return Err(WriteError::ChangedSince {
                    file: r.file.clone(),
                    entry: String::new(),
                });
            }
        }
        for r in &planned {
            let lines = secret_lines(&r.file, &r.before);
            if !lines.is_empty() {
                return Err(WriteError::Secret {
                    file: r.file.clone(),
                    lines,
                });
            }
        }
        // The whole migrated project adds no error (as for an accept).
        let mut after = disk.clone();
        for r in &planned {
            after.insert(r.file.clone(), r.after.clone());
        }
        if let Some((file, _)) = &removal {
            after.remove(file);
        }
        let new_errors = added_errors(self.errors_exact(disk)?, self.errors_exact(after)?);
        if let Some(first) = new_errors.first() {
            return Err(WriteError::Invalid {
                file: first.file.clone(),
                findings: new_errors,
            });
        }
        let mut entries: Vec<HistoryEntry> = Vec::with_capacity(planned.len() + 1);
        let stop = |file: &str, cause: WriteError, written: bool, entries: Vec<HistoryEntry>| {
            if entries.is_empty() && !written {
                return cause;
            }
            WriteError::MigrationStopped {
                file: file.to_string(),
                migrated: entries.into_iter().map(|e| e.file).collect(),
                written,
                cause: Box::new(cause),
            }
        };
        for r in planned {
            if let Err(e) = self.unchanged(&r.file, &r.before) {
                return Err(stop(&r.file, e, false, entries));
            }
            let after_sha256 = sha256_hex(&r.after);
            match self.write_step(
                actor.clone(),
                &r.file,
                Change {
                    kind: HistoryKind::Migration,
                    ref_entry: None,
                    before: r.before,
                    after: r.after,
                    audit: Vec::new(),
                },
            ) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    let written = std::fs::read(self.root.join(&r.file))
                        .is_ok_and(|b| sha256_hex(&b) == after_sha256);
                    return Err(stop(&r.file, e, written, entries));
                }
            }
        }
        if let Some((file, before)) = removal {
            match self.remove_step(actor, &file, before) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    let written = !self.root.join(&file).exists();
                    return Err(stop(&file, e, written, entries));
                }
            }
        }
        Ok(entries)
    }

    /// A removal's intent, unlink and commit (story 2.13): the intent is
    /// committed first; the file is read once more, and a change since
    /// refuses with nothing removed; a crash after the unlink leaves the
    /// intent, which reconcile completes (the file is absent).
    fn remove_step(
        &self,
        actor: Actor,
        file: &str,
        before: Vec<u8>,
    ) -> Result<HistoryEntry, WriteError> {
        let empty = sha256_hex(b"");
        let intent = NewIntent {
            file: file.to_string(),
            kind: HistoryKind::Removal,
            ref_entry: None,
            before_sha256: sha256_hex(&before),
            after_sha256: empty.clone(),
            diff: linediff::unified(file, &before, b""),
            before: before.clone(),
            after: Vec::new(),
            audit: Vec::new(),
        };
        for p in self.history.pending_intents()? {
            if p.intent.file == file {
                self.settle(p)?;
            }
        }
        // Resolved again (exact name, no link) right before the read and
        // the unlink: a link put in its place is never removed as the file.
        self.resolve(file)?;
        let id = self.history.begin_intent(actor.clone(), intent)?;
        match self.read(file) {
            Ok(now) if now == before => {}
            Ok(_) => {
                let _ = self.history.drop_intent(actor, id);
                return Err(WriteError::ChangedOnDisk {
                    files: vec![file.to_string()],
                });
            }
            Err(e) => {
                // Nothing was removed: the intent goes too.
                let _ = self.history.drop_intent(actor, id);
                return Err(e);
            }
        }
        self.remember(file, &empty);
        #[cfg(feature = "test-support")]
        if self.failpoint(FailPoint::BeforeRename) {
            return Err(WriteError::Interrupted(
                "simulated crash before the file was removed".into(),
            ));
        }
        let path = self.root.join(file);
        if let Err(e) = self.resolve(file) {
            let _ = self.history.drop_intent(actor, id);
            return Err(e);
        }
        if let Err(e) = std::fs::remove_file(&path) {
            let _ = self.history.drop_intent(actor, id);
            return Err(WriteError::Io {
                context: format!("cannot remove {file}"),
                source: e,
            });
        }
        let dir = path.parent().unwrap_or(&self.root).to_path_buf();
        std::fs::File::open(&dir)
            .and_then(|d| d.sync_all())
            .map_err(io(format!("cannot sync the folder of {file}")))?;
        #[cfg(feature = "test-support")]
        if self.failpoint(FailPoint::AfterRename) {
            return Err(WriteError::Interrupted(
                "simulated crash after the file was removed".into(),
            ));
        }
        Ok(self.history.commit_entry(actor, id)?)
    }

    /// Story 2.13 (`accounts migrate --rollback`; AD-15, AD-16): re-creates
    /// the project's old `models/accounts.yaml` holding exactly `bytes` (a
    /// backup's, never emitted canonically), as a `create` entry placed
    /// without replacing anything, for the owner only and only in a
    /// project. Refused, with nothing written, when anything stands at that
    /// name or the bytes hold a secret-looking value. The project's check
    /// is not run: the file is an error by design (it was the project's
    /// state before the migration) and grants nothing.
    pub fn restore_legacy_accounts(
        &self,
        actor: Actor,
        bytes: Vec<u8>,
    ) -> Result<HistoryEntry, WriteError> {
        if !matches!(actor, Actor::Human { .. }) {
            return Err(WriteError::NotOwner {
                actor: actor.describe(),
            });
        }
        if self.user {
            return Err(WriteError::BadEdit(format!(
                "the writer of this Mac's Accounts file never writes {LEGACY_ACCOUNTS_FILE}; nothing was written"
            )));
        }
        let _one = self.serial()?;
        let file = LEGACY_ACCOUNTS_FILE.to_string();
        self.plain_config_path(&file)?;
        for p in self.history.pending_intents()? {
            if p.intent.file == file {
                self.settle(p)?;
            }
        }
        self.resolve_absent(&file)?;
        let lines = secret_lines(&file, &bytes);
        if !lines.is_empty() {
            return Err(WriteError::BadEdit(format!(
                "the {file} to restore holds a secret-looking value (line {}); nothing was written",
                Joined(&lines)
            )));
        }
        if bytes.is_empty() {
            return Err(WriteError::NoChange(format!(
                "the {file} to restore would be empty"
            )));
        }
        let after_sha256 = sha256_hex(&bytes);
        let intent = NewIntent {
            file: file.clone(),
            kind: HistoryKind::Create,
            ref_entry: None,
            before_sha256: sha256_hex(b""),
            after_sha256: after_sha256.clone(),
            diff: linediff::unified(&file, b"", &bytes),
            before: Vec::new(),
            after: bytes.clone(),
            audit: Vec::new(),
        };
        let id = self.history.begin_intent(actor.clone(), intent)?;
        self.remember(&file, &after_sha256);
        match self.place(&file, &bytes) {
            Ok(()) => {}
            Err(Written::No(e)) => {
                let _ = self.history.drop_intent(actor, id);
                self.forget_own_writes(&file);
                return Err(e);
            }
            Err(Written::Maybe(e)) => return Err(e),
        }
        Ok(self.history.commit_entry(actor, id)?)
    }

    /// `ChangedSince` (for a migration) unless `file` still holds `before`.
    fn unchanged(&self, file: &str, before: &[u8]) -> Result<(), WriteError> {
        if self.read(file)? == before {
            Ok(())
        } else {
            Err(WriteError::ChangedSince {
                file: file.to_string(),
                entry: String::new(),
            })
        }
    }

    /// AD-16's startup rule: completes each pending intent whose file holds
    /// its new bytes (audited as reconciled by `actor`) and drops the rest.
    pub fn reconcile(&self, actor: Actor) -> Result<ReconcileReport, WriteError> {
        let _one = self.serial()?;
        let mut report = ReconcileReport {
            completed: Vec::new(),
            dropped: Vec::new(),
        };
        for p in self.history.pending_intents()? {
            let id = p.id.clone();
            match self.settle_as(actor.clone(), p)? {
                Some(entry) => report.completed.push(entry),
                None => report.dropped.push(id),
            }
        }
        Ok(report)
    }

    /// Settles one pending intent by the writer itself (`System(config-writer)`).
    fn settle(&self, p: PendingIntent) -> Result<Option<HistoryEntry>, WriteError> {
        let system = Actor::System {
            component: "config-writer".into(),
        };
        self.settle_as(system, p)
    }

    /// Completes the intent when its file holds the new bytes (the entry),
    /// otherwise drops it (`None`).
    fn settle_as(
        &self,
        actor: Actor,
        p: PendingIntent,
    ) -> Result<Option<HistoryEntry>, WriteError> {
        let path = self.root.join(&p.intent.file);
        let holds_new = self.kind_of(&p.intent.file).is_some()
            && if p.intent.kind == HistoryKind::Removal {
                // A removal is written once the file is gone (story 2.13).
                matches!(std::fs::symlink_metadata(&path), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
            } else {
                std::fs::read(&path).is_ok_and(|b| sha256_hex(&b) == p.intent.after_sha256)
            };
        if holds_new {
            let entry = self.history.reconcile_intent(actor, p.id)?;
            self.remember(&entry.file, &entry.after_sha256);
            Ok(Some(entry))
        } else {
            self.history.drop_intent(actor, p.id)?;
            Ok(None)
        }
    }

    /// True when (file, sha256) is one of this writer's own writes.
    pub fn is_own_write(&self, file: &str, sha256: &str) -> bool {
        self.own
            .lock()
            .is_ok_and(|own| own.iter().any(|(f, h)| f == file && h == sha256))
    }

    /// Forgets the fingerprints of this writer's writes to `file` (story
    /// 1.9): the session calls it once a write returned, so they cover only
    /// writes in flight and a later outside save of the same bytes is
    /// announced.
    pub(crate) fn forget_own_writes(&self, file: &str) {
        if let Ok(mut own) = self.own.lock() {
            own.retain(|(f, _)| f != file);
        }
    }

    /// This writer's recent writes as (file, sha256), oldest first.
    pub fn own_writes(&self) -> Vec<(String, String)> {
        self.own
            .lock()
            .map(|own| own.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn remember(&self, file: &str, sha256: &str) {
        if let Ok(mut own) = self.own.lock() {
            if own.len() == OWN_WRITES {
                own.pop_front();
            }
            own.push_back((file.to_string(), sha256.to_string()));
        }
    }

    fn entry(&self, id: &str) -> Result<HistoryEntry, WriteError> {
        self.history
            .entry(id.to_string())?
            .ok_or_else(|| WriteError::UnknownEntry(id.to_string()))
    }

    /// The kind of the config file `file` names, by its plain path inside
    /// the project (not absolute, no `..`), whether or not it exists.
    fn plain_config_path(&self, file: &str) -> Result<FileKind, WriteError> {
        let refuse = |reason: &str| WriteError::BadPath {
            file: file.to_string(),
            reason: reason.to_string(),
        };
        if file.is_empty() {
            return Err(refuse("no file was named"));
        }
        if file.starts_with('/') || Path::new(file).is_absolute() {
            return Err(refuse(
                "an absolute path; name a config file by its path inside the project",
            ));
        }
        if file.split(['/', '\\']).any(|c| c == "..") {
            return Err(refuse("a path with `..`; name a file inside the project"));
        }
        self.kind_of(file).ok_or_else(|| refuse(self.not_written()))
    }

    /// Checks that a create may place `file` (already a plain config path):
    /// nothing stands at its name (by any spelling of it, so a
    /// case-insensitive volume does not open another file), each parent
    /// folder that exists is a real folder with that exact name and no link,
    /// and the rest can be made.
    fn resolve_absent(&self, file: &str) -> Result<(), WriteError> {
        let refuse = |reason: String| WriteError::BadPath {
            file: file.to_string(),
            reason,
        };
        let parts: Vec<&str> = file.split('/').collect();
        let mut path = self.root.clone();
        for (i, c) in parts.iter().enumerate() {
            let last = i + 1 == parts.len();
            let listed: Vec<String> = std::fs::read_dir(&path)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            let lower = c.to_lowercase();
            let on_disk = listed
                .iter()
                .find(|n| n.as_str() == *c)
                .or_else(|| listed.iter().find(|n| n.to_lowercase() == lower));
            path.push(c);
            let Some(on_disk) = on_disk else {
                // Neither this component nor anything below it exists.
                return Ok(());
            };
            if last {
                return Err(refuse(if on_disk == c {
                    "the file already exists; a create never replaces a file (edit it instead)"
                        .into()
                } else {
                    format!(
                        "the file already exists as '{on_disk}'; a create never replaces a file"
                    )
                }));
            }
            if on_disk != c {
                return Err(refuse(format!(
                    "the folder's name on disk is '{on_disk}'; name it with its exact spelling"
                )));
            }
            match std::fs::symlink_metadata(&path) {
                Ok(m) if m.file_type().is_symlink() => {
                    return Err(refuse(
                        "a parent folder is a link; the writer writes inside the project only"
                            .into(),
                    ));
                }
                Ok(m) if !m.is_dir() => {
                    return Err(refuse(format!("'{c}' is not a folder")));
                }
                Ok(_) => {}
                Err(_) => return Ok(()),
            }
        }
        Ok(())
    }

    /// The kind of an existing config file named by `file`, or a refusal.
    fn resolve(&self, file: &str) -> Result<FileKind, WriteError> {
        let refuse = |reason: &str| WriteError::BadPath {
            file: file.to_string(),
            reason: reason.to_string(),
        };
        if file.is_empty() {
            return Err(refuse("no file was named"));
        }
        if file.starts_with('/') || Path::new(file).is_absolute() {
            return Err(refuse(
                "an absolute path; name a config file by its path inside the project",
            ));
        }
        if file.split(['/', '\\']).any(|c| c == "..") {
            return Err(refuse("a path with `..`; name a file inside the project"));
        }
        let kind = self
            .kind_of(file)
            .ok_or_else(|| refuse(self.not_written()))?;
        // Each component must be the exact name listed in its folder (a
        // case-insensitive volume would open `ALPHA.md` as `alpha.md`), and
        // none may be a link: the write stays inside the project.
        let mut path = self.root.clone();
        for c in file.split('/') {
            let listed: Vec<String> = std::fs::read_dir(&path)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            exact_name(&listed, c).map_err(|reason| refuse(&reason))?;
            path.push(c);
            match std::fs::symlink_metadata(&path) {
                Ok(m) if m.file_type().is_symlink() => {
                    return Err(refuse(
                        "a link; the writer edits files inside the project only",
                    ));
                }
                Ok(_) => {}
                Err(_) => {
                    return Err(refuse(
                        "the file does not exist; the writer edits existing files only",
                    ));
                }
            }
        }
        if !path.is_file() {
            return Err(refuse("not a file"));
        }
        Ok(kind)
    }

    fn read(&self, file: &str) -> Result<Vec<u8>, WriteError> {
        std::fs::read(self.root.join(file)).map_err(io(format!("cannot read {file}")))
    }

    /// Checks the project with `after` in place of `file`, then runs the
    /// intent, write and commit sequence.
    fn step(&self, actor: Actor, file: &str, change: Change) -> Result<HistoryEntry, WriteError> {
        Self::refuse_agent(&actor)?;
        self.step_checks(file, &change.before, &change.after)?;
        self.write_step(actor, file, change)
    }

    /// [`ConfigWriter::step`]'s checks of a change from `before` to `after`.
    fn step_checks(&self, file: &str, before: &[u8], after: &[u8]) -> Result<(), WriteError> {
        // No key may reach History, an intent or a diff through the bytes
        // the file holds now: the owner removes it by hand first.
        let lines = secret_lines_of(self.kind_of(file), file, before);
        if !lines.is_empty() {
            return Err(WriteError::Secret {
                file: file.to_string(),
                lines,
            });
        }
        // Owner decision (2026-11-21): refused only when the change adds an
        // error the project does not already have (errors after ⊆ errors
        // before), so a project broken in several files is fixed one file
        // at a time. Errors are compared without their line, which an edit
        // may shift.
        let errors_now = self.errors(BTreeMap::new())?;
        let errors_after = self.errors(BTreeMap::from([(file.to_string(), after.to_vec())]))?;
        let new_errors = added_errors(errors_now, errors_after);
        if !new_errors.is_empty() {
            return Err(WriteError::Invalid {
                file: file.to_string(),
                findings: new_errors,
            });
        }
        Ok(())
    }

    /// The intent, write and commit sequence of a change already checked
    /// (the secret check of the current bytes runs again). Refused with
    /// `ChangedOnDisk`, writing nothing, when the file no longer holds
    /// `before` once the intent is recorded.
    fn write_step(
        &self,
        actor: Actor,
        file: &str,
        change: Change,
    ) -> Result<HistoryEntry, WriteError> {
        Self::refuse_agent(&actor)?;
        let Change {
            kind,
            ref_entry,
            before,
            after,
            audit,
        } = change;
        let lines = secret_lines_of(self.kind_of(file), file, &before);
        if !lines.is_empty() {
            return Err(WriteError::Secret {
                file: file.to_string(),
                lines,
            });
        }
        let after_sha256 = sha256_hex(&after);
        let intent = NewIntent {
            file: file.to_string(),
            kind,
            ref_entry,
            before_sha256: sha256_hex(&before),
            after_sha256: after_sha256.clone(),
            diff: linediff::unified(file, &before, &after),
            before,
            after,
            audit,
        };
        let bytes = intent.after.clone();
        // An intent of this file left pending while the writer ran is settled
        // first, as at the next start; this write changes the file's hash.
        for p in self.history.pending_intents()? {
            if p.intent.file == file {
                self.settle(p)?;
            }
        }
        let before_now = intent.before.clone();
        let id = self.history.begin_intent(actor.clone(), intent)?;
        // The file is read again here, after the (slow) validation and the
        // durable intent: a save made outside the app since the change was
        // built is never overwritten or left out of History, and the intent
        // goes (refactor sweep, deferred from story 1.10's review).
        if self.read(file)? != before_now {
            let _ = self.history.drop_intent(actor, id);
            return Err(WriteError::ChangedOnDisk {
                files: vec![file.to_string()],
            });
        }
        // Remembered before the rename, so the watcher never sees it as outside.
        self.remember(file, &after_sha256);
        match self.replace(file, &bytes) {
            Ok(()) => {}
            Err(Written::No(e)) => {
                // Nothing reached the file: the intent goes (best effort; a
                // left intent is dropped by the next reconcile).
                let _ = self.history.drop_intent(actor, id);
                return Err(e);
            }
            Err(Written::Maybe(e)) => return Err(e),
        }
        Ok(self.history.commit_entry(actor, id)?)
    }

    /// The project's error findings, with `overrides` read instead of disk.
    fn errors(&self, overrides: BTreeMap<String, Vec<u8>>) -> Result<Vec<Finding>, WriteError> {
        if self.user {
            let mut files = self.scan();
            files.extend(overrides);
            return Ok(user_errors(&files));
        }
        let loaded = load_with(&self.root, overrides).map_err(|e| match e {
            LoadError::NotFound(p) | LoadError::NotADirectory(p) => WriteError::BadPath {
                file: p.display().to_string(),
                reason: "the project folder is gone".into(),
            },
        })?;
        Ok(loaded
            .findings
            .into_iter()
            .filter(|f| f.severity == Severity::Error)
            .collect())
    }

    /// The project's error findings when its tracked files are exactly
    /// `files` (see [`load_exact`]).
    fn errors_exact(&self, files: BTreeMap<String, Vec<u8>>) -> Result<Vec<Finding>, WriteError> {
        if self.user {
            return Ok(user_errors(&files));
        }
        let loaded =
            load_exact(&self.root, files, LoadOptions::default()).map_err(|e| match e {
                LoadError::NotFound(p) | LoadError::NotADirectory(p) => WriteError::BadPath {
                    file: p.display().to_string(),
                    reason: "the project folder is gone".into(),
                },
            })?;
        Ok(loaded
            .findings
            .into_iter()
            .filter(|f| f.severity == Severity::Error)
            .collect())
    }

    /// The tracked files as they are on disk: the project's, or (user mode)
    /// the user Accounts file when it exists.
    fn scan(&self) -> BTreeMap<String, Vec<u8>> {
        if self.user {
            return std::fs::read(self.root.join(USER_ACCOUNTS_FILE))
                .ok()
                .map(|b| BTreeMap::from([(USER_ACCOUNTS_FILE.to_string(), b)]))
                .unwrap_or_default();
        }
        crate::session::scan(&self.root)
    }

    /// The options of a new temp file: owner-only (0600) in user mode.
    fn new_file(&self) -> std::fs::OpenOptions {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        if self.user {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        o
    }

    /// A create's placement (AD-16): missing parent folders, a new temp
    /// file, fsync, a hard link to the final name (which fails when the name
    /// exists), removal of the temp file, fsync of the folder.
    fn place(&self, file: &str, bytes: &[u8]) -> Result<(), Written> {
        let path = self.root.join(file);
        let dir = path.parent().unwrap_or(&self.root).to_path_buf();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tmp = dir.join(format!(".{name}{TMP_SUFFIX}"));
        let written = (|| -> Result<(), WriteError> {
            // Each missing parent is made one folder at a time, so a link
            // that appeared meanwhile is not followed without being seen.
            let mut at = self.root.clone();
            let parents: Vec<&str> = file.split('/').collect();
            for c in &parents[..parents.len().saturating_sub(1)] {
                at.push(c);
                match std::fs::symlink_metadata(&at) {
                    Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
                    Ok(_) => {
                        return Err(WriteError::BadPath {
                            file: file.to_string(),
                            reason: format!(
                                "'{c}' is not a folder (a file or a link); the writer writes inside the project only"
                            ),
                        });
                    }
                    Err(_) => {
                        std::fs::create_dir(&at)
                            .map_err(io(format!("cannot create the folder {}", at.display())))?;
                        // The new folder's entry is made durable in its
                        // parent, as the file's is in its folder below.
                        let parent = at.parent().unwrap_or(&self.root);
                        std::fs::File::open(parent)
                            .and_then(|d| d.sync_all())
                            .map_err(io(format!("cannot sync the folder of {}", at.display())))?;
                    }
                }
            }
            match std::fs::symlink_metadata(&tmp) {
                Ok(m) if m.is_dir() => std::fs::remove_dir_all(&tmp),
                Ok(_) => std::fs::remove_file(&tmp),
                Err(_) => Ok(()),
            }
            .map_err(io(format!("cannot remove {}", tmp.display())))?;
            let mut f = self
                .new_file()
                .open(&tmp)
                .map_err(io(format!("cannot create {}", tmp.display())))?;
            f.write_all(bytes)
                .map_err(io(format!("cannot write {}", tmp.display())))?;
            f.sync_all()
                .map_err(io(format!("cannot sync {}", tmp.display())))?;
            Ok(())
        })();
        if let Err(e) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(Written::No(e));
        }
        #[cfg(feature = "test-support")]
        if self.failpoint(FailPoint::BeforeRename) {
            return Err(Written::Maybe(WriteError::Interrupted(
                "simulated crash before the file was placed".into(),
            )));
        }
        let linked = std::fs::hard_link(&tmp, &path);
        #[cfg(feature = "test-support")]
        let linked = if self.failpoint(FailPoint::NoHardLinks) {
            // Undo the link the test volume allowed, then refuse as a
            // volume without hard links does.
            if linked.is_ok() {
                let _ = std::fs::remove_file(&path);
            }
            Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
        } else {
            linked
        };
        if let Err(e) = linked {
            let _ = std::fs::remove_file(&tmp);
            return Err(Written::No(
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    WriteError::BadPath {
                    file: file.to_string(),
                    reason: "the file already exists (it appeared while the create ran); a create never replaces a file, so it was left as it is".into(),
                }
                } else {
                    WriteError::BadEdit(format!(
                        "{file} cannot be placed by a hard link ({e}); a new config file is only ever placed that way, so that it can never replace a file, and this volume refuses hard links. Nothing was written: create the file by hand, or keep the project on a volume that supports hard links"
                    ))
                },
            ));
        }
        // The file stands under its name from here on: a failure leaves the
        // intent for the next reconcile.
        std::fs::remove_file(&tmp)
            .map_err(io(format!("cannot remove {}", tmp.display())))
            .map_err(Written::Maybe)?;
        std::fs::File::open(&dir)
            .and_then(|d| d.sync_all())
            .map_err(io(format!("cannot sync the folder of {file}")))
            .map_err(Written::Maybe)?;
        #[cfg(feature = "test-support")]
        if self.failpoint(FailPoint::AfterRename) {
            return Err(Written::Maybe(WriteError::Interrupted(
                "simulated crash after the file was placed".into(),
            )));
        }
        Ok(())
    }

    #[cfg(feature = "test-support")]
    fn failpoint(&self, at: FailPoint) -> bool {
        self.fail.lock().is_ok_and(|f| *f == Some(at))
    }

    /// Temp file, mode, fsync, rename, fsync of the folder.
    fn replace(&self, file: &str, bytes: &[u8]) -> Result<(), Written> {
        let path = self.root.join(file);
        let dir = path.parent().unwrap_or(&self.root).to_path_buf();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tmp = dir.join(format!(".{name}{TMP_SUFFIX}"));
        let written = (|| -> Result<(), WriteError> {
            #[allow(unused_mut)]
            let mut perms = std::fs::metadata(&path)
                .map_err(io(format!("cannot read {file}")))?
                .permissions();
            #[cfg(unix)]
            if self.user {
                use std::os::unix::fs::PermissionsExt;
                perms.set_mode(0o600);
            }
            // Whatever stands at the temp path (a file, a link, a folder) goes
            // first, and the temp file is created new: a link is never followed.
            match std::fs::symlink_metadata(&tmp) {
                Ok(m) if m.is_dir() => std::fs::remove_dir_all(&tmp),
                Ok(_) => std::fs::remove_file(&tmp),
                Err(_) => Ok(()),
            }
            .map_err(io(format!("cannot remove {}", tmp.display())))?;
            let mut f = self
                .new_file()
                .open(&tmp)
                .map_err(io(format!("cannot create {}", tmp.display())))?;
            f.write_all(bytes)
                .map_err(io(format!("cannot write {}", tmp.display())))?;
            f.set_permissions(perms)
                .map_err(io(format!("cannot set the mode of {}", tmp.display())))?;
            f.sync_all()
                .map_err(io(format!("cannot sync {}", tmp.display())))?;
            Ok(())
        })();
        if let Err(e) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(Written::No(e));
        }
        #[cfg(feature = "test-support")]
        if self.failpoint(FailPoint::BeforeRename) {
            return Err(Written::Maybe(WriteError::Interrupted(
                "simulated crash before the rename".into(),
            )));
        }
        if let Err(e) = std::fs::rename(&tmp, &path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(Written::No(WriteError::Io {
                context: format!("cannot replace {file}"),
                source: e,
            }));
        }
        std::fs::File::open(&dir)
            .and_then(|d| d.sync_all())
            .map_err(io(format!("cannot sync the folder of {file}")))
            .map_err(Written::Maybe)?;
        #[cfg(feature = "test-support")]
        if self.failpoint(FailPoint::AfterRename) {
            return Err(Written::Maybe(WriteError::Interrupted(
                "simulated crash after the rename".into(),
            )));
        }
        Ok(())
    }
}

/// The errors of the user Accounts file in `files` (none when it is absent).
fn user_errors(files: &BTreeMap<String, Vec<u8>>) -> Vec<Finding> {
    files
        .get(USER_ACCOUNTS_FILE)
        .map(|bytes| crate::user_accounts::load_bytes(USER_ACCOUNTS_FILE, bytes).findings)
        .unwrap_or_default()
        .into_iter()
        .filter(|f| f.severity == Severity::Error)
        .collect()
}

/// `text` with `line` inserted after its first top-level `schema_version:`
/// line, ending as that line ends (a last line without one gets the file's
/// line ending before the new line). `None` without such a line.
fn insert_after_schema_version(text: &str, line: &str) -> Option<String> {
    let crlf = text.contains("\r\n");
    let mut out = String::with_capacity(text.len() + line.len() + 2);
    let mut done = false;
    for l in text.split_inclusive('\n') {
        out.push_str(l);
        if done {
            continue;
        }
        let is_version = l
            .strip_prefix("schema_version")
            .is_some_and(|rest| rest.trim_start_matches([' ', '\t']).starts_with(':'));
        if !is_version {
            continue;
        }
        let ending = if l.ends_with("\r\n") {
            "\r\n"
        } else if l.ends_with('\n') {
            "\n"
        } else {
            let e = if crlf { "\r\n" } else { "\n" };
            out.push_str(e);
            e
        };
        out.push_str(line);
        out.push_str(ending);
        done = true;
    }
    done.then_some(out)
}

/// Refuses an Undo or Restore of a `removal` entry (story 2.13): the one
/// way back is `agentic accounts migrate --rollback`, which re-creates the
/// file byte for byte from its backup.
fn refuse_removal(entry: &HistoryEntry, action: &str) -> Result<(), WriteError> {
    if entry.kind != HistoryKind::Removal {
        return Ok(());
    }
    Err(WriteError::BadEdit(format!(
        "{}: entry {} removed the file when its Accounts moved to this Mac, so an {action} of it is refused; put the file back with `agentic accounts migrate --project <folder> --rollback`, which re-creates it from its backup",
        entry.file, entry.id
    )))
}

/// Refuses an Undo or Restore of an outside edit that would create or
/// delete its file (story 1.9): in an `outside_edit` entry an empty side
/// means the file was absent, and the writer never creates or deletes a
/// config file. `undo` writes `before` back, `restore` writes `after`.
fn refuse_absent(entry: &HistoryEntry, undo: bool) -> Result<(), WriteError> {
    if entry.kind != HistoryKind::OutsideEdit {
        return Ok(());
    }
    let action = if undo { "undo" } else { "restore" };
    let target = if undo { &entry.before } else { &entry.after };
    let words = if target.is_empty() {
        "would delete the file; the app never deletes a config file, so delete it by hand"
    } else if undo && entry.after.is_empty() {
        "would create the file again; the app never creates a config file, so create it by hand (this entry holds its earlier content)"
    } else {
        return Ok(());
    };
    Err(WriteError::BadEdit(format!(
        "{}: entry {} recorded a change made outside the app, and to {action} it {words}",
        entry.file, entry.id
    )))
}

/// One change to an existing file, on its way through the intent, write and
/// commit sequence.
struct Change {
    kind: HistoryKind,
    /// The entry an Undo or Restore acts on.
    ref_entry: Option<String>,
    before: Vec<u8>,
    after: Vec<u8>,
    /// Audit entries the History port writes with the commit (story 2.1).
    audit: Vec<AuditNote>,
}

/// A failed replace: `No` when the file kept its bytes for certain, `Maybe`
/// when the intent must stay for the next reconcile.
enum Written {
    No(WriteError),
    Maybe(WriteError),
}

/// The folders that hold config files (where temp files can be left).
fn config_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![root.to_path_buf(), root.join("models")];
    dirs.extend(FOLDERS.iter().map(|(f, _)| root.join(f)));
    if let Ok(read) = std::fs::read_dir(root.join("skills")) {
        let mut skills: Vec<PathBuf> = read
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path())
            .collect();
        skills.sort();
        dirs.extend(skills);
    }
    dirs
}

/// The canonical bytes of the config file `file` holding `bytes`: what the
/// writer would write for an edit that changes nothing.
pub fn canonical_bytes(file: &str, bytes: &[u8]) -> Result<Vec<u8>, WriteError> {
    let user = (file == USER_ACCOUNTS_FILE).then_some(FileKind::Accounts);
    let kind = user
        .or_else(|| canon::kind_of(file))
        .ok_or_else(|| WriteError::BadPath {
            file: file.to_string(),
            reason: "not a config file the app writes".into(),
        })?;
    let text = std::str::from_utf8(bytes).map_err(|_| WriteError::NotEditable {
        file: file.to_string(),
        reason: "it is not UTF-8 text".into(),
    })?;
    let doc = canon::parse(kind, text).map_err(|reason| WriteError::NotEditable {
        file: file.to_string(),
        reason,
    })?;
    canon::emit(&doc)
        .map(String::into_bytes)
        .map_err(|e| WriteError::BadEdit(format!("{file}: {e}")))
}

/// Every config file of the project at `root` that the writer may edit, as
/// sorted project-relative paths.
pub fn config_files(root: &Path) -> Vec<String> {
    let mut out = BTreeSet::new();
    let mut add = |rel: String| {
        if canon::kind_of(&rel).is_some() && root.join(&rel).is_file() {
            out.insert(rel);
        }
    };
    add("company.yaml".into());
    add("tools.yaml".into());
    for m in MODEL_FILES {
        add(format!("models/{m}.yaml"));
    }
    for (folder, _) in FOLDERS {
        if let Ok(read) = std::fs::read_dir(root.join(folder)) {
            for e in read.flatten() {
                add(format!("{folder}/{}", e.file_name().to_string_lossy()));
            }
        }
    }
    if let Ok(read) = std::fs::read_dir(root.join("skills")) {
        for e in read.flatten() {
            add(format!(
                "skills/{}/SKILL.md",
                e.file_name().to_string_lossy()
            ));
        }
    }
    out.into_iter().collect()
}

/// How `wanted` matches the names listed in its folder: `Ok` for an exact
/// entry, otherwise the refusal's words (naming the on-disk name when only
/// the case differs).
fn exact_name(listed: &[String], wanted: &str) -> Result<(), String> {
    if listed.iter().any(|n| n == wanted) {
        return Ok(());
    }
    let lower = wanted.to_lowercase();
    match listed.iter().find(|n| n.to_lowercase() == lower) {
        Some(on_disk) => Err(format!(
            "the name on disk is '{on_disk}'; name the file with its exact spelling"
        )),
        None => Err("the file does not exist; the writer edits existing files only".into()),
    }
}

/// The errors in `after` that `before` does not account for, matched as a
/// multiset on (file, message, fix): the line is left out because an edit
/// may move an error it did not cause.
pub(crate) fn added_errors(before: Vec<Finding>, after: Vec<Finding>) -> Vec<Finding> {
    let mut known: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for f in before {
        *known.entry((f.file, f.message, f.fix)).or_default() += 1;
    }
    after
        .into_iter()
        .filter(
            |f| match known.get_mut(&(f.file.clone(), f.message.clone(), f.fix.clone())) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    false
                }
                _ => true,
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::exact_name;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn exact_name_needs_the_listed_spelling() {
        let listed = names(&["alpha.md", "beta.md"]);
        assert_eq!(exact_name(&listed, "alpha.md"), Ok(()));
        let case = exact_name(&listed, "ALPHA.md").unwrap_err();
        assert!(case.contains("'alpha.md'"), "{case}");
        assert!(case.contains("exact"), "{case}");
        let missing = exact_name(&listed, "gamma.md").unwrap_err();
        assert!(missing.contains("does not exist"), "{missing}");
    }
}
