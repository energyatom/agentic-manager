//! Config migrations (story 1.10, R4, AD-7, AD-16): config files at an older
//! `schema_version` are stepped forward only on the owner's accept.
//!
//! The registry is an ordered list of steps, each from one version to the
//! next; a step is a pure change on the file's parsed tree, after which the
//! runner sets `schema_version` to the step's target and the result is
//! emitted canonically (a seat's Markdown body byte for byte).
//!
//! [`migration_offers`] reads the project and offers every file older than
//! [`crate::checks::SCHEMA_VERSION`] that has a path of steps to it, as a
//! [`MigrationOffer`] with its unified diff; it writes nothing. A project with
//! an offer builds no snapshot ([`crate::ConfigSession`] refuses it with
//! `MigrationPending`), so nothing runs on unaccepted content.
//! [`accept_migrations`] writes every offered file through the
//! [`ConfigWriter`] as one `migration` History entry each, or refuses before
//! writing any; only the owner (an `Actor::Human`) accepts. A file holding a
//! secret-looking value, a linked file, or one a step refuses, is not offered
//! (the listing follows the loader: a link it skips is not listed at all): its load
//! finding says to remove the value, or gives the step's reason, and to fix
//! it by hand; an accept that would leave any older file with a step path
//! behind is refused before anything is written. A file older than the
//! first step, or with no path, stays a load error whose fix says the app
//! cannot migrate it; a newer file is refused as before.
//!
//! The shipped registry is empty: `SCHEMA_VERSION` is still 1. The
//! `test-support` feature registers a step from 0 to 1 that renames a seat's
//! v0-only `group:` key to `team:` and bumps the version (other files get
//! only the bump), to prove the path end to end. Accept writes an offer only
//! when it is exactly the app's migration of the file's current bytes.

use std::collections::BTreeMap;
use std::path::Path;

use am_domain::{Actor, HistoryEntry};

use crate::canon::{self, FileKind};
use crate::finding::Report;
use crate::node::{Kind, Node};
use crate::writer::{secret_lines, sha256_hex};
use crate::{ConfigWriter, LoadError, WriteError, checks, linediff};

/// One registered step: from `from` to `from + 1`.
struct Step {
    from: i64,
    /// The change on the parsed tree (the runner sets `schema_version`).
    change: fn(FileKind, &mut Node) -> Result<(), String>,
}

/// The shipped registry (no step yet; story 1.14 adds the first).
#[cfg(not(feature = "test-support"))]
const STEPS: &[Step] = &[];

/// Tests only: a step from 0 to 1 that renames a seat's v0-only `group:`
/// key to `team:` (its team reference); any other file only gets the
/// version bump.
#[cfg(feature = "test-support")]
const STEPS: &[Step] = &[Step {
    from: 0,
    change: group_to_team,
}];

#[cfg(feature = "test-support")]
fn group_to_team(kind: FileKind, tree: &mut Node) -> Result<(), String> {
    if kind != FileKind::Seat {
        return Ok(());
    }
    let Kind::Map(entries) = &mut tree.kind else {
        return Ok(());
    };
    if !entries.iter().any(|(k, _)| k.name == "group") {
        return Ok(());
    }
    if entries.iter().any(|(k, _)| k.name == "team") {
        return Err("the seat has both `group` and `team`; keep one by hand".into());
    }
    for (k, _) in entries.iter_mut() {
        if k.name == "group" {
            k.name = "team".into();
        }
    }
    Ok(())
}

/// The steps from `from` to `SCHEMA_VERSION`, or `None` when there is no
/// path (older than the first step, a gap, or not older at all).
fn path_from(from: i64) -> Option<Vec<&'static Step>> {
    if from >= checks::SCHEMA_VERSION {
        return None;
    }
    let mut out = Vec::new();
    let mut at = from;
    while at < checks::SCHEMA_VERSION {
        let step = STEPS.iter().find(|s| s.from == at)?;
        out.push(step);
        at += 1;
    }
    Some(out)
}

/// True when a file at `from` (older than `SCHEMA_VERSION`) can be migrated.
pub(crate) fn has_path(from: i64) -> bool {
    path_from(from).is_some()
}

/// A config file the app offers to step to the current `schema_version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOffer {
    /// Project-relative path with forward slashes, e.g. `seats/alpha.md`.
    pub file: String,
    /// The file's `schema_version` now.
    pub from: i64,
    /// The version it is stepped to (this app's).
    pub to: i64,
    /// Lowercase hex sha256 of the file's bytes when offered; the accept is
    /// refused when the file no longer holds them.
    pub before_sha256: String,
    /// Unified diff from the file's bytes to `after`.
    pub diff: String,
    /// The canonical bytes the accept writes.
    pub after: Vec<u8>,
}

/// The top-level `schema_version` of `tree` when it is a whole number.
fn version_of(tree: &Node) -> Option<i64> {
    tree.as_map()?
        .iter()
        .find(|(k, _)| k.name == "schema_version")
        .and_then(|(_, v)| match v.kind {
            Kind::Int(n) => Some(n),
            _ => None,
        })
}

/// Why an older file that has a step path is not offered.
pub(crate) enum Blocked {
    /// Its bytes hold a secret-looking value, which the diff would show.
    Secret,
    /// The file, or a folder on its way (the project-relative path given),
    /// is a link the loader reads through; the writer never writes through
    /// links.
    Link(String),
    /// A step (or the canonical emit) refused it; the reason in words,
    /// never the file's content.
    Step(String),
}

/// What the app can do with one config file's migration.
pub(crate) enum Assessed {
    /// Not older: current, newer, a SKILL.md, or not readable as a config
    /// file (the loader reports that).
    NotOlder,
    /// Older, with no path of steps to this app's version.
    NoPath(i64),
    /// Older, with a path, but not offered.
    Blocked(Blocked),
    Offer(MigrationOffer),
}

/// The first component of `file` under `root` that is a link (`seats` for
/// a linked folder, the file itself for a linked file), if any.
fn linked(root: &Path, file: &str) -> Option<String> {
    let mut path = root.to_path_buf();
    let mut rel: Vec<&str> = Vec::new();
    for c in file.split('/') {
        path.push(c);
        rel.push(c);
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Some(rel.join("/"));
        }
    }
    None
}

/// How the config file `file` (under `root`) holding `bytes` stands for
/// migration.
pub(crate) fn assess(root: &Path, file: &str, bytes: &[u8]) -> Assessed {
    let Some(kind) = canon::kind_of(file) else {
        return Assessed::NotOlder;
    };
    // A SKILL.md frontmatter carries no schema_version (Agent Skills format).
    if kind == FileKind::Skill {
        return Assessed::NotOlder;
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Assessed::NotOlder;
    };
    let Ok(mut doc) = canon::parse(kind, text) else {
        return Assessed::NotOlder;
    };
    let Some(from) = version_of(&doc.tree) else {
        return Assessed::NotOlder;
    };
    if from >= checks::SCHEMA_VERSION {
        return Assessed::NotOlder;
    }
    let Some(steps) = path_from(from) else {
        return Assessed::NoPath(from);
    };
    if let Some(link) = linked(root, file) {
        return Assessed::Blocked(Blocked::Link(link));
    }
    // A key-looking value would reach the diff and the stepped bytes: the
    // owner removes it by hand first (the load finding says so).
    if !secret_lines(file, bytes).is_empty() {
        return Assessed::Blocked(Blocked::Secret);
    }
    for step in steps {
        if let Err(why) = (step.change)(kind, &mut doc.tree) {
            return Assessed::Blocked(Blocked::Step(why));
        }
        let to = serde_json::Value::from(step.from + 1);
        if let Err(why) = canon::apply(&mut doc.tree, &["schema_version".to_string()], Some(&to)) {
            return Assessed::Blocked(Blocked::Step(why));
        }
    }
    let after = match canon::emit(&doc) {
        Ok(text) => text.into_bytes(),
        Err(why) => return Assessed::Blocked(Blocked::Step(why)),
    };
    Assessed::Offer(MigrationOffer {
        file: file.to_string(),
        from,
        to: checks::SCHEMA_VERSION,
        before_sha256: sha256_hex(bytes),
        diff: linediff::unified(file, bytes, &after),
        after,
    })
}

/// The offer for the config file `file` holding `bytes`, or `None` when it
/// is not older or cannot be offered.
fn offer_for(root: &Path, file: &str, bytes: &[u8]) -> Option<MigrationOffer> {
    match assess(root, file, bytes) {
        Assessed::Offer(offer) => Some(offer),
        _ => None,
    }
}

/// Rewords the fix of each older file's schema_version error when the file
/// has a step path but is not offered: it holds a secret-looking value, or
/// a step refused it (the step's reason). The loader's `files` are the
/// bytes it read.
pub(crate) fn word_blocked(
    root: &Path,
    reports: &mut BTreeMap<String, Report>,
    files: &BTreeMap<String, Vec<u8>>,
) {
    let offered = checks::migration_fix();
    for (file, rep) in reports.iter_mut() {
        let Some(bytes) = files.get(file) else {
            continue;
        };
        if !rep.findings.iter().any(|f| f.fix == offered) {
            continue;
        }
        let fix = match assess(root, file, bytes) {
            Assessed::Blocked(Blocked::Secret) => checks::migration_blocked_fix(),
            Assessed::Blocked(Blocked::Link(link)) => checks::migration_link_fix(file, &link),
            Assessed::Blocked(Blocked::Step(why)) => checks::migration_failed_fix(&why),
            _ => continue,
        };
        for f in rep.findings.iter_mut().filter(|f| f.fix == offered) {
            f.fix = fix.clone();
        }
    }
}

/// Words for each older file of `files` that has a step path and is not in
/// `accepted`, whatever the reason: an accept that leaves one behind would
/// half-migrate the project.
pub(crate) fn left_behind(
    root: &Path,
    files: &BTreeMap<String, Vec<u8>>,
    accepted: &BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    files
        .iter()
        .filter(|(file, _)| !accepted.contains_key(*file))
        .filter_map(|(file, bytes)| match assess(root, file, bytes) {
            Assessed::Offer(_) => Some(format!(
                "{file} waits for its migration but is not in this accept; accept every offer at once"
            )),
            Assessed::Blocked(why) => Some(blocked_words(file, &why)),
            _ => None,
        })
        .collect()
}

/// Words for each older file of `files` that has a step path but cannot be
/// offered (the session names them instead of asking for an accept).
pub(crate) fn blocked(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    files
        .iter()
        .filter_map(|(file, bytes)| match assess(root, file, bytes) {
            Assessed::Blocked(why) => Some(blocked_words(file, &why)),
            _ => None,
        })
        .collect()
}

/// What keeps `file` from being offered, and what to do.
fn blocked_words(file: &str, why: &Blocked) -> String {
    match why {
        Blocked::Secret => format!(
            "{file} holds a secret-looking value; remove it by hand, then ask for the offers again"
        ),
        Blocked::Link(link) if link == file => format!(
            "{file} is a link; the app does not write through links. Replace it with the file itself, then ask for the offers again"
        ),
        Blocked::Link(link) => format!(
            "{link}/ is a link (it holds {file}); the app does not write through links. Replace the link with the folder itself, then ask for the offers again"
        ),
        Blocked::Step(reason) => format!(
            "{file}: the app cannot migrate it: {reason}; fix it by hand, then ask for the offers again"
        ),
    }
}

/// Each older file of `files` with no step path, as `<file> (schema_version
/// <n>)`: they stay errors after an accept.
pub(crate) fn no_path(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    files
        .iter()
        .filter_map(|(file, bytes)| match assess(root, file, bytes) {
            Assessed::NoPath(n) => Some(format!("{file} (schema_version {n})")),
            _ => None,
        })
        .collect()
}

/// Words for each older file of `files` that has no step path.
pub(crate) fn unmigratable(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    files
        .iter()
        .filter_map(|(file, bytes)| match assess(root, file, bytes) {
            Assessed::NoPath(n) => Some(format!(
                "{file} is at schema_version {n}, which this app cannot migrate; rewrite it by hand"
            )),
            _ => None,
        })
        .collect()
}

/// The offers for a map of project-relative paths to bytes (the config
/// files among them), sorted by file.
pub(crate) fn offers_in(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> Vec<MigrationOffer> {
    files
        .iter()
        .filter_map(|(file, bytes)| offer_for(root, file, bytes))
        .collect()
}

/// The config files of the project at `root` as the loader reads them
/// (the session's scan): a link inside an entity folder is skipped, as the
/// loader skips it; a linked top-level or model file is read (and then
/// blocked from migration as a link).
pub(crate) fn migration_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    crate::session::scan(root)
        .into_iter()
        .filter(|(file, _)| canon::kind_of(file).is_some())
        .collect()
}

/// Every config file of the project at `root` that is older than this app's
/// `schema_version` and can be stepped to it, one offer each, sorted by
/// file. Reads only; nothing is written.
pub fn migration_offers(root: &Path) -> Result<Vec<MigrationOffer>, LoadError> {
    if !root.exists() {
        return Err(LoadError::NotFound(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(LoadError::NotADirectory(root.to_path_buf()));
    }
    Ok(offers_in(root, &migration_files(root)))
}

/// The owner's accept of `offers` by `actor`: every offered file is written
/// through `writer` as one `migration` History entry (in the offers' order),
/// or none is. Refused when no file is older, when the offers do not cover
/// every older file that has a step path (named, with why), when a file changed since it was offered, or
/// when the migrated project would have an error it does not have now.
pub fn accept_migrations(
    writer: &ConfigWriter,
    actor: Actor,
    offers: Vec<MigrationOffer>,
) -> Result<Vec<HistoryEntry>, WriteError> {
    writer.migrate(actor, offers)
}

/// Refuses an offer unless it is exactly the app's migration of the file's
/// current bytes `before` (same versions, stepped bytes and diff): accept writes
/// only what the registered steps produce, never other content.
pub(crate) fn check_offer(
    root: &Path,
    offer: &MigrationOffer,
    before: &[u8],
) -> Result<(), WriteError> {
    match offer_for(root, &offer.file, before) {
        Some(own)
            if own.from == offer.from
                && own.to == offer.to
                && own.after == offer.after
                && own.diff == offer.diff =>
        {
            Ok(())
        }
        _ => Err(WriteError::BadEdit(format!(
            "{}: the offer does not match the app's migration of this file; ask for the migration offers again",
            offer.file
        ))),
    }
}
