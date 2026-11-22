//! Story 2.13 (AD-15 *Migrate*, AD-16, AD-17 *Old project files*): what
//! `agentic accounts migrate` needs from the config files.
//!
//! - [`ConfigMigration`] is the one command that moves a project's old
//!   `models/accounts.yaml` out: its [`MigrationStep`]s rename an Account id
//!   in this project's references (`--as <old>=<new>`) and remove that one
//!   file. [`crate::ConfigWriter::apply_migration`] applies every step or
//!   none, for the owner only; removal is reachable only there, and only
//!   for `models/accounts.yaml`.
//! - [`account_renames`] computes what the rename steps would write (each
//!   file, the lines renamed, its bytes before and after), without writing:
//!   the command names the renamed lines in its sentence and records their
//!   hashes in the backup's manifest before the migration is applied.
//! - [`legacy_accounts`] reads the old file's entries, as the user Accounts
//!   file's loader reads that file (its findings never repeat a value from
//!   the file), with the lines that hold a secret-looking value.

use std::collections::BTreeMap;
use std::path::Path;

use crate::canon::{self, FileKind};
use crate::finding::{Finding, Severity};
use crate::node::{Kind, Node};
use crate::user_accounts::{LEGACY_ACCOUNTS_FILE, UserAccounts};
use crate::writer::{WriteError, secret_lines, sha256_hex};
use crate::{AccountUse, checks};

/// One owner-accepted migration of a project's config files (AD-7, AD-16):
/// every step is applied, or none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigMigration {
    pub steps: Vec<MigrationStep>,
}

/// One step of a [`ConfigMigration`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationStep {
    /// Removes `file`, which must still hold the bytes whose sha256 is
    /// `before_sha256`. Only `models/accounts.yaml` is ever removed.
    Remove { file: String, before_sha256: String },
    /// Every reference to the Account `old` in this project (a seat's
    /// `model` and `fallback`, a capability's `provider`, the search
    /// provider, a Source's `account`) names `new` instead.
    RenameAccount { old: String, new: String },
    /// A precondition, not a write: the exact set of files the renames may
    /// write (project-relative path to the sha256 of the bytes the owner
    /// was shown), an empty set included. When given, the migration is
    /// refused with nothing written if the renames planned from disk would
    /// write any file outside it, or a file in it no longer holds its
    /// bytes. `agentic accounts migrate` always gives it.
    Expects { files: BTreeMap<String, String> },
}

/// One file a rename step rewrites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamedFile {
    /// Project-relative path with forward slashes.
    pub file: String,
    /// The file lines renamed, as they were before (each named reference).
    pub lines: Vec<u32>,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

impl RenamedFile {
    pub fn before_sha256(&self) -> String {
        sha256_hex(&self.before)
    }

    pub fn after_sha256(&self) -> String {
        sha256_hex(&self.after)
    }
}

/// What the rename steps `renames` (old, new; applied in order) would
/// write in the project at `root`, as it is on disk now; sorted by file.
pub fn account_renames(
    root: &Path,
    renames: &[(String, String)],
) -> Result<Vec<RenamedFile>, WriteError> {
    plan_renames(root, &crate::session::scan(root), renames)
}

/// [`account_renames`] over the tracked files `files` (project-relative
/// path to bytes).
pub(crate) fn plan_renames(
    root: &Path,
    files: &BTreeMap<String, Vec<u8>>,
    renames: &[(String, String)],
) -> Result<Vec<RenamedFile>, WriteError> {
    let mut now = files.clone();
    let mut out: BTreeMap<String, RenamedFile> = BTreeMap::new();
    for (old, new) in renames {
        check_rename(old, new)?;
        let uses = crate::account_edit::account_uses_exact(root, old, now.clone());
        let mut by_file: BTreeMap<String, Vec<u32>> = BTreeMap::new();
        for AccountUse { file, line, .. } in uses {
            by_file.entry(file).or_default().push(line);
        }
        for (file, lines) in by_file {
            let Some(before) = now.get(&file).cloned() else {
                continue;
            };
            let after = rename_in(&file, &before, old, new, &lines)?;
            let entry = out.entry(file.clone()).or_insert_with(|| RenamedFile {
                file: file.clone(),
                lines: Vec::new(),
                before: before.clone(),
                after: Vec::new(),
            });
            entry.lines.extend(lines);
            entry.lines.sort_unstable();
            entry.lines.dedup();
            entry.after = after.clone();
            now.insert(file, after);
        }
    }
    Ok(out.into_values().filter(|r| r.after != r.before).collect())
}

/// Refuses a rename whose new id is not one an Account can have.
fn check_rename(old: &str, new: &str) -> Result<(), WriteError> {
    if !checks::is_kebab(new) {
        return Err(WriteError::BadEdit(format!(
            "'{new}' is not an Account id: use lowercase letters, digits and single hyphens (kebab-case); nothing was written"
        )));
    }
    if crate::refs::RESERVED_ACCOUNTS.contains(&new) {
        return Err(WriteError::BadEdit(format!(
            "'{new}' is a reserved local Account id; nothing was written"
        )));
    }
    if old == new {
        return Err(WriteError::BadEdit(format!(
            "a rename of '{old}' needs another id; nothing was written"
        )));
    }
    Ok(())
}

/// `bytes` (the config file `file`) with the reference `old` on each of
/// `lines` (file lines) renamed to `new`, emitted canonically; the Markdown
/// body is kept byte for byte.
fn rename_in(
    file: &str,
    bytes: &[u8],
    old: &str,
    new: &str,
    lines: &[u32],
) -> Result<Vec<u8>, WriteError> {
    let kind = canon::kind_of(file).ok_or_else(|| WriteError::BadPath {
        file: file.to_string(),
        reason: "not a config file the app writes".into(),
    })?;
    let text = std::str::from_utf8(bytes).map_err(|_| WriteError::NotEditable {
        file: file.to_string(),
        reason: "it is not UTF-8 text".into(),
    })?;
    let mut doc = canon::parse(kind, text).map_err(|reason| WriteError::NotEditable {
        file: file.to_string(),
        reason,
    })?;
    // A Markdown file's frontmatter starts on its line 2.
    let offset = u32::from(kind.markdown());
    let mut renamed = Vec::new();
    rename_nodes(&mut doc.tree, old, new, offset, lines, &mut renamed);
    let mut missing: Vec<u32> = lines
        .iter()
        .copied()
        .filter(|l| !renamed.contains(l))
        .collect();
    missing.dedup();
    if !missing.is_empty() {
        return Err(WriteError::NotEditable {
            file: file.to_string(),
            reason: format!(
                "the reference to '{old}' on line {} could not be renamed",
                missing
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    canon::emit(&doc)
        .map(String::into_bytes)
        .map_err(|e| WriteError::BadEdit(format!("{file}: {e}")))
}

/// Renames each `account:` or `provider:` value `old` whose file line is
/// one of `lines`; records the lines renamed.
fn rename_nodes(
    node: &mut Node,
    old: &str,
    new: &str,
    offset: u32,
    lines: &[u32],
    renamed: &mut Vec<u32>,
) {
    match &mut node.kind {
        Kind::Map(pairs) => {
            for (key, value) in pairs.iter_mut() {
                let line = value.line + offset;
                let is_ref = matches!(key.name.as_str(), "account" | "provider")
                    && matches!(&value.kind, Kind::Str(s) if s == old)
                    && lines.contains(&line);
                if is_ref {
                    value.kind = Kind::Str(new.to_string());
                    value.raw = None;
                    renamed.push(line);
                } else {
                    rename_nodes(value, old, new, offset, lines, renamed);
                }
            }
        }
        Kind::Seq(items) => {
            for item in items.iter_mut() {
                rename_nodes(item, old, new, offset, lines, renamed);
            }
        }
        _ => {}
    }
}

/// The old `models/accounts.yaml` as read for a migration.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyAccounts {
    /// The Accounts it declares with an id and a known provider, in file
    /// order.
    pub accounts: UserAccounts,
    /// Every entry's id as written, in file order (`None`: no id).
    pub ids: Vec<Option<String>>,
    /// Its errors, each `line N: message`; no value from the file is
    /// repeated.
    pub errors: Vec<String>,
    /// The lines that hold a secret-looking value (never shown).
    pub secret_lines: Vec<u32>,
    /// The lines that hold an error.
    pub error_lines: Vec<u32>,
    /// Each item of `accounts:` in file order, with whether it may be
    /// shown.
    pub entries: Vec<LegacyEntry>,
}

/// One item of the old file's `accounts:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyEntry {
    /// The item's place in `accounts:` (from 0).
    pub index: usize,
    /// The line the item starts on.
    pub line: u32,
    /// Its id, only when it is a valid Account id that does not look like
    /// a key (the membership rule of the user file's messages): the only
    /// value of an item with an error ever shown.
    pub id: Option<String>,
    /// No error and no secret-looking value on any of its lines.
    pub clean: bool,
}

impl LegacyAccounts {
    /// True when every item with this id is clean (and there is one) and
    /// its id may be shown: an id that appears on any unclean item shows no
    /// value of any of its items.
    pub fn is_clean(&self, id: &str) -> bool {
        let named: Vec<&LegacyEntry> = self
            .entries
            .iter()
            .filter(|e| self.ids.get(e.index).and_then(Option::as_deref) == Some(id))
            .collect();
        !named.is_empty() && named.iter().all(|e| e.clean && e.id.as_deref() == Some(id))
    }
}

/// Reads the old per-project Accounts file's `bytes` (story 2.13).
pub fn legacy_accounts(bytes: &[u8]) -> LegacyAccounts {
    let loaded = crate::user_accounts::load_bytes(LEGACY_ACCOUNTS_FILE, bytes);
    let errors = loaded
        .findings
        .iter()
        .filter(|f: &&Finding| f.severity == Severity::Error)
        .map(|f| format!("line {}: {}", f.line, f.message))
        .collect();
    let secrets = secret_lines(LEGACY_ACCOUNTS_FILE, bytes);
    let error_lines: Vec<u32> = loaded
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .map(|f| f.line)
        .collect();
    let mut ids = Vec::new();
    let mut entries = Vec::new();
    if let Ok(text) = std::str::from_utf8(bytes)
        && let Ok(doc) = canon::parse(FileKind::Accounts, text)
        && let Some(Kind::Seq(items)) = doc
            .tree
            .as_map()
            .and_then(|m| m.iter().find(|(k, _)| k.name == "accounts"))
            .map(|(_, v)| &v.kind)
    {
        ids = items
            .iter()
            .map(|item| {
                item.as_map()
                    .and_then(|m| m.iter().find(|(k, _)| k.name == "id"))
                    .and_then(|(_, v)| match &v.kind {
                        Kind::Str(s) => Some(s.clone()),
                        _ => None,
                    })
            })
            .collect();
        entries = items
            .iter()
            .zip(&ids)
            .enumerate()
            .map(|(index, (item, id))| {
                let (first, last) = crate::account_edit::lines_of(item);
                let within = |l: &u32| (first..=last).contains(l);
                let clean = !secrets.iter().any(within) && !error_lines.iter().any(within);
                let id = id
                    .clone()
                    .filter(|id| checks::is_kebab(id) && !checks::has_secret_prefix(id));
                LegacyEntry {
                    index,
                    line: first,
                    id,
                    clean,
                }
            })
            .collect();
    }
    LegacyAccounts {
        accounts: loaded.accounts,
        ids,
        errors,
        secret_lines: secrets,
        error_lines,
        entries,
    }
}
