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
//! It never creates, deletes or renames a config file.
//!
//! [`ConfigWriter::assign_identity`] (a [`ConfigIdentityAssign`], story 1.13,
//! AD-32) is the one write that does not emit canonically: it inserts a
//! single `project_id: <id>` line directly after `schema_version` and leaves
//! every other byte of `company.yaml` as it was (comments, key order, blank
//! lines and line endings), through the same intent, rename and commit, so
//! `reconcile` settles a crash after the rename. It commits History and its
//! audit only, never an approved grant set (AD-16).
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
    Actor, ConfigHistory, HistoryEntry, HistoryError, HistoryKind, NewIntent, PendingIntent,
};
use sha2::{Digest, Sha256};

use crate::canon::{self, FileKind};
use crate::finding::Report;
use crate::migrate::{self, MigrationOffer};
use crate::{FOLDERS, Finding, LoadError, MODEL_FILES, Severity, checks, linediff, load_with};

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
    /// After the intent and the temp file, before the rename.
    BeforeRename,
    /// After the rename, before History is committed.
    AfterRename,
}

/// Why a write was refused or did not finish.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("refused: {file}: {reason}")]
    BadPath { file: String, reason: String },
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
    /// yet reloaded), so the write never takes that change in silently.
    #[error(
        "refused: {} changed on disk outside the app; Reload (or undo that change) first, then make the edit",
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
    let text = String::from_utf8_lossy(bytes);
    let mut rep = Report::new(file);
    checks::text_secrets(&mut rep, &text, 1);
    if let Some(kind) = canon::kind_of(file)
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

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The project's config writer. Build one per open project.
pub struct ConfigWriter {
    root: PathBuf,
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
            history,
            serial: Mutex::new(()),
            own: Mutex::new(VecDeque::new()),
            #[cfg(feature = "test-support")]
            fail: Mutex::new(None),
        })
    }

    /// Stops the next writes at `point` as a crash would (tests only).
    #[cfg(feature = "test-support")]
    pub fn fail_at(&self, point: Option<FailPoint>) {
        if let Ok(mut f) = self.fail.lock() {
            *f = point;
        }
    }

    fn serial(&self) -> Result<std::sync::MutexGuard<'_, ()>, WriteError> {
        self.serial
            .lock()
            .map_err(|_| WriteError::Interrupted("an earlier write panicked".into()))
    }

    /// Applies `edit` by `actor` and returns its History entry.
    pub fn apply(&self, actor: Actor, edit: ConfigEdit) -> Result<HistoryEntry, WriteError> {
        let _one = self.serial()?;
        let kind = self.resolve(&edit.file)?;
        let file = edit.file;
        let before = self.read(&file)?;
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
        self.step(actor, &file, HistoryKind::Edit, None, before, after)
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
            HistoryKind::Edit,
            None,
            before,
            after.into_bytes(),
        )
    }

    /// Writes entry `id`'s before-bytes back, as a new `undo` entry. Refused
    /// when the file no longer holds that entry's after-bytes.
    pub fn undo(&self, actor: Actor, id: &str) -> Result<HistoryEntry, WriteError> {
        let _one = self.serial()?;
        let entry = self.entry(id)?;
        refuse_absent(&entry, true)?;
        self.resolve(&entry.file)?;
        let current = self.read(&entry.file)?;
        if sha256_hex(&current) != entry.after_sha256 {
            return Err(WriteError::ChangedSince {
                file: entry.file,
                entry: entry.id,
            });
        }
        self.step(
            actor,
            &entry.file,
            HistoryKind::Undo,
            Some(entry.id.clone()),
            current,
            entry.before,
        )
    }

    /// Writes entry `id`'s after-bytes back, as a new `restore` entry.
    pub fn restore(&self, actor: Actor, id: &str) -> Result<HistoryEntry, WriteError> {
        let _one = self.serial()?;
        let entry = self.entry(id)?;
        refuse_absent(&entry, false)?;
        self.resolve(&entry.file)?;
        let current = self.read(&entry.file)?;
        if sha256_hex(&current) == entry.after_sha256 {
            return Err(WriteError::NoChange(format!(
                "{} already holds what entry {} wrote",
                entry.file, entry.id
            )));
        }
        self.step(
            actor,
            &entry.file,
            HistoryKind::Restore,
            Some(entry.id.clone()),
            current,
            entry.after,
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
            migrate::check_offer(&self.root, offer, &before)?;
            overrides.insert(offer.file.clone(), offer.after.clone());
            befores.push(before);
        }
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
                HistoryKind::Migration,
                None,
                before,
                offer.after.clone(),
            ) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
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
        let holds_new = canon::kind_of(&p.intent.file).is_some()
            && std::fs::read(self.root.join(&p.intent.file))
                .is_ok_and(|b| sha256_hex(&b) == p.intent.after_sha256);
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
        let kind = canon::kind_of(file).ok_or_else(|| {
            refuse(
                "not a config file the app writes (company.yaml, tools.yaml, models/, seats/, \
                 skills/<id>/SKILL.md, workflows/, tables/, dashboards/, sources/ or schemas/)",
            )
        })?;
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
    fn step(
        &self,
        actor: Actor,
        file: &str,
        kind: HistoryKind,
        ref_entry: Option<String>,
        before: Vec<u8>,
        after: Vec<u8>,
    ) -> Result<HistoryEntry, WriteError> {
        // No key may reach History, an intent or a diff through the bytes
        // the file holds now: the owner removes it by hand first.
        let lines = secret_lines(file, &before);
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
        let errors_after = self.errors(BTreeMap::from([(file.to_string(), after.clone())]))?;
        let new_errors = added_errors(errors_now, errors_after);
        if !new_errors.is_empty() {
            return Err(WriteError::Invalid {
                file: file.to_string(),
                findings: new_errors,
            });
        }
        self.write_step(actor, file, kind, ref_entry, before, after)
    }

    /// The intent, write and commit sequence of a change already checked
    /// (the secret check of the current bytes runs again).
    fn write_step(
        &self,
        actor: Actor,
        file: &str,
        kind: HistoryKind,
        ref_entry: Option<String>,
        before: Vec<u8>,
        after: Vec<u8>,
    ) -> Result<HistoryEntry, WriteError> {
        let lines = secret_lines(file, &before);
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
        };
        let bytes = intent.after.clone();
        // An intent of this file left pending while the writer ran is settled
        // first, as at the next start; this write changes the file's hash.
        for p in self.history.pending_intents()? {
            if p.intent.file == file {
                self.settle(p)?;
            }
        }
        let id = self.history.begin_intent(actor.clone(), intent)?;
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
            let perms = std::fs::metadata(&path)
                .map_err(io(format!("cannot read {file}")))?
                .permissions();
            // Whatever stands at the temp path (a file, a link, a folder) goes
            // first, and the temp file is created new: a link is never followed.
            match std::fs::symlink_metadata(&tmp) {
                Ok(m) if m.is_dir() => std::fs::remove_dir_all(&tmp),
                Ok(_) => std::fs::remove_file(&tmp),
                Err(_) => Ok(()),
            }
            .map_err(io(format!("cannot remove {}", tmp.display())))?;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
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
    let kind = canon::kind_of(file).ok_or_else(|| WriteError::BadPath {
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
