//! Config migrations (story 1.10, R4, AD-7, AD-16): config files at an older
//! `schema_version` are stepped forward only on the owner's accept.
//!
//! The registry is an ordered list of steps, each from one version to the
//! next. Since story 1.14 a step sees the whole project (moving hosts across
//! files cannot be expressed per file): it changes the parsed trees of the
//! files at its version and may read every other file, after which the
//! runner sets each stepped file's `schema_version` to the step's target and
//! the result is emitted canonically (a seat's Markdown body byte for byte).
//! A step may refuse one file (its reason in words) and attach notes to a
//! file's offer (an entry it dropped, say).
//!
//! [`migration_offers`] reads the project and offers every file older than
//! [`crate::checks::SCHEMA_VERSION`] that has a path of steps to it, as a
//! [`MigrationOffer`] with its unified diff and notes; it writes nothing. A
//! project with an offer builds no snapshot ([`crate::ConfigSession`]
//! refuses it with `MigrationPending`), so nothing runs on unaccepted
//! content. [`accept_migrations`] writes every offered file through the
//! [`ConfigWriter`] as one `migration` History entry each, or refuses before
//! writing any; only the owner (an `Actor::Human`) accepts. A file holding a
//! secret-looking value, a linked file, or one a step refuses, is not
//! offered (the listing follows the loader: a link it skips is not listed at
//! all): its load finding says to remove the value, or gives the step's
//! reason, and to fix it by hand; an accept that would leave any older file
//! with a step path behind is refused before anything is written. A file
//! older than the first step, or with no path, stays a load error whose fix
//! says the app cannot migrate it; a newer file is refused as before.
//!
//! The shipped registry holds one step, 1→2 (story 1.14, AD-48): `tools.yaml`
//! `egress_hosts` moves into the `allowed_hosts` of every seat that can
//! fetch. The `test-support` feature also registers a step from 0 to 1 that
//! renames a seat's v0-only `group:` key to `team:` (other files get only the
//! bump), to prove the path end to end. Accept writes an offer only when it
//! is exactly the app's migration of the whole project's current bytes.

use std::collections::BTreeMap;
use std::path::Path;

use am_domain::capability::Capability;
use am_domain::{Actor, HistoryEntry, HostPattern};

use crate::canon::{self, Doc, FileKind};
use crate::finding::Report;
use crate::node::{Key, Kind, Node};
use crate::writer::{secret_lines, sha256_hex};
use crate::{ConfigWriter, LoadError, WriteError, checks, linediff};

/// One config file's parsed tree while the project is migrated.
pub(crate) struct ProjectFile {
    pub kind: FileKind,
    pub doc: Doc,
    /// The `schema_version` the tree is at now.
    pub at: i64,
    /// True when this migration steps the file (older, with a step path,
    /// neither linked nor holding a secret-looking value). Other files are
    /// read for context only.
    pub stepping: bool,
    /// Words for the file's offer.
    pub notes: Vec<String>,
    /// Set when a step refuses the file: why, in words.
    pub refused: Option<String>,
}

impl ProjectFile {
    /// True when the step from `from` changes this file.
    fn steps_at(&self, from: i64) -> bool {
        self.stepping && self.refused.is_none() && self.at == from
    }
}

/// The project's config files (every readable one, by project-relative
/// path) while it is migrated.
pub(crate) struct ProjectTrees {
    pub files: BTreeMap<String, ProjectFile>,
}

/// One registered step: from `from` to `from + 1`.
struct Step {
    from: i64,
    /// The change on the project's trees: only files that
    /// [`ProjectFile::steps_at`] `from` may be changed (the runner then sets
    /// their `schema_version`); any file may be read.
    change: fn(&mut ProjectTrees, i64),
}

/// The shipped registry: 1→2 moves `egress_hosts` (story 1.14).
#[cfg(not(feature = "test-support"))]
const STEPS: &[Step] = &[Step {
    from: 1,
    change: egress_hosts_to_seats,
}];

/// Tests: also a step from 0 to 1 that renames a seat's v0-only `group:`
/// key to `team:` (its team reference); any other file only gets the
/// version bump.
#[cfg(feature = "test-support")]
const STEPS: &[Step] = &[
    Step {
        from: 0,
        change: group_to_team,
    },
    Step {
        from: 1,
        change: egress_hosts_to_seats,
    },
];

#[cfg(feature = "test-support")]
fn group_to_team(project: &mut ProjectTrees, from: i64) {
    for file in project.files.values_mut() {
        if file.kind != FileKind::Seat || !file.steps_at(from) {
            continue;
        }
        let Kind::Map(entries) = &mut file.doc.tree.kind else {
            continue;
        };
        if !entries.iter().any(|(k, _)| k.name == "group") {
            continue;
        }
        if entries.iter().any(|(k, _)| k.name == "team") {
            file.refused = Some("the seat has both `group` and `team`; keep one by hand".into());
            continue;
        }
        for (k, _) in entries.iter_mut() {
            if k.name == "group" {
                k.name = "team".into();
            }
        }
    }
}

const TOOLS_FILE: &str = "tools.yaml";

/// The top-level entry `key` of a tree.
fn top<'n>(tree: &'n Node, key: &str) -> Option<&'n Node> {
    tree.as_map()?
        .iter()
        .find(|(k, _)| k.name == key)
        .map(|(_, v)| v)
}

/// A scalar's text as written.
fn scalar_text(node: &Node) -> Option<String> {
    match &node.kind {
        Kind::Str(s) => Some(s.clone()),
        Kind::Int(_) | Kind::Float(_) | Kind::Bool(_) => node.raw.clone(),
        _ => None,
    }
}

/// True when a seat's tree switches `capability` on.
fn capability_on(tree: &Node, capability: Capability) -> bool {
    top(tree, "capabilities")
        .and_then(|caps| top(caps, capability.id()))
        .and_then(|c| top(c, "on"))
        .is_some_and(|on| on.kind == Kind::Bool(true))
}

/// The seat id of a seat file's path (`seats/<id>.md`).
fn seat_id(file: &str) -> Option<&str> {
    file.strip_prefix("seats/")?.strip_suffix(".md")
}

/// The 1→2 step (story 1.14, AD-48): copies each `tools.yaml`
/// `egress_hosts` entry that `HostPattern` accepts into the `allowed_hosts`
/// of every seat that can fetch (Web search, Browse & read pages or Crawl
/// on, or owning a Source of any type) and of no other seat, merged without
/// duplicates; drops and lists each entry `HostPattern` refuses; removes the
/// key. It gives no seat a host it could not already reach.
fn egress_hosts_to_seats(project: &mut ProjectTrees, from: i64) {
    let Some(tools) = project.files.get_mut(TOOLS_FILE) else {
        return;
    };
    if !tools.steps_at(from) {
        return;
    }
    let Kind::Map(entries) = &mut tools.doc.tree.kind else {
        return;
    };
    let Some(pos) = entries.iter().position(|(k, _)| k.name == "egress_hosts") else {
        return;
    };
    let (_, listed) = entries.remove(pos);
    let mut hosts: Vec<HostPattern> = Vec::new();
    let mut notes = Vec::new();
    let items: Vec<&Node> = match &listed.kind {
        Kind::Seq(items) => items.iter().collect(),
        Kind::Null => Vec::new(),
        _ => vec![&listed],
    };
    for item in items {
        let Some(text) = scalar_text(item) else {
            notes.push(format!(
                "an egress_hosts entry that is {} (not a host) was dropped",
                item.type_name()
            ));
            continue;
        };
        match HostPattern::parse(&text) {
            Ok(p) if hosts.contains(&p) => {}
            Ok(p) => hosts.push(p),
            Err(e) => notes.push(format!(
                "egress_hosts entry '{text}' was dropped: {e}; add the host name to a seat's allowed_hosts by hand if a seat needs it"
            )),
        }
    }

    // Which seats can fetch: a fetching capability on, or the owner of any
    // Source (read from every file, whatever its version).
    let mut owners: Vec<String> = Vec::new();
    for (file, f) in &project.files {
        if f.kind == FileKind::Source
            && file.starts_with("sources/")
            && let Some(owner) = top(&f.doc.tree, "owner_seat").and_then(scalar_text)
        {
            owners.push(owner);
        }
    }
    let fetches = |file: &str, f: &ProjectFile| {
        seat_id(file).is_some_and(|id| owners.iter().any(|o| o == id))
            || [Capability::WebSearch, Capability::Browse, Capability::Crawl]
                .into_iter()
                .any(|c| capability_on(&f.doc.tree, c))
    };
    let listed_words = hosts
        .iter()
        .map(HostPattern::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let mut fetching = 0;
    if !hosts.is_empty() {
        for (file, f) in project.files.iter_mut() {
            if f.kind != FileKind::Seat || !fetches(file, f) {
                continue;
            }
            fetching += 1;
            let id = seat_id(file).unwrap_or(file).to_string();
            if !f.steps_at(from) {
                notes.push(format!(
                    "seat '{id}' can fetch but is not stepped by this migration; add {listed_words} to its allowed_hosts by hand if it needs them"
                ));
                continue;
            }
            let added = match merge_hosts(&mut f.doc.tree, &hosts) {
                Ok(added) => added,
                Err(why) => {
                    // Blocked, never stepped without its hosts: tools.yaml
                    // would lose them while the seat kept none, so the
                    // all-or-nothing accept waits for the owner's fix.
                    f.refused = Some(format!(
                        "seat '{id}' can fetch, so it must be given {listed_words} from tools.yaml egress_hosts, but {why}; make allowed_hosts a list of host patterns"
                    ));
                    continue;
                }
            };
            if !added.is_empty() {
                f.notes.push(format!(
                    "copies {} from tools.yaml egress_hosts into allowed_hosts, because this seat can fetch",
                    added.join(", ")
                ));
            }
        }
        if fetching == 0 {
            notes.push(format!(
                "no seat can fetch (Web search, Browse & read pages or Crawl on, or owning a Source), so {listed_words} reached no seat"
            ));
        }
    }
    if let Some(tools) = project.files.get_mut(TOOLS_FILE) {
        tools.notes.extend(notes);
    }
}

/// Appends each of `hosts` that the seat's `allowed_hosts` does not list
/// yet (compared as patterns), creating the key when absent; returns the
/// patterns added, or why none could be (the key holds something other than
/// a list, which is left for the owner).
fn merge_hosts(tree: &mut Node, hosts: &[HostPattern]) -> Result<Vec<String>, String> {
    let Kind::Map(entries) = &mut tree.kind else {
        return Err("the seat's frontmatter is not key: value pairs".into());
    };
    let pos = match entries.iter().position(|(k, _)| k.name == "allowed_hosts") {
        Some(p) => p,
        None => {
            entries.push((
                Key {
                    name: "allowed_hosts".into(),
                    line: 0,
                },
                Node {
                    line: 0,
                    kind: Kind::Seq(Vec::new()),
                    raw: None,
                },
            ));
            entries.len() - 1
        }
    };
    let list = &mut entries[pos].1;
    if list.kind == Kind::Null {
        list.kind = Kind::Seq(Vec::new());
    }
    let type_name = list.type_name();
    let Kind::Seq(items) = &mut list.kind else {
        // Not a list: left for the owner, whose load reports it.
        return Err(format!("its allowed_hosts holds {type_name}, not a list"));
    };
    let present: Vec<HostPattern> = items
        .iter()
        .filter_map(scalar_text)
        .filter_map(|t| HostPattern::parse(&t).ok())
        .collect();
    let mut added = Vec::new();
    for h in hosts {
        if present.contains(h) {
            continue;
        }
        items.push(Node {
            line: 0,
            kind: Kind::Str(h.to_string()),
            raw: None,
        });
        added.push(h.to_string());
    }
    Ok(added)
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
    /// Words about this file's migration for the owner (story 1.14): an
    /// entry a step dropped, hosts a seat gains; empty for a plain bump.
    pub notes: Vec<String>,
    /// Lowercase hex sha256 of every config file the migration was derived
    /// from, by path (story 1.14: a step reads the whole project, so the
    /// stepped bytes depend on files that are not offered, such as another
    /// seat's capabilities or a Source's `owner_seat`). The accept is
    /// refused when any of them changed, was removed or was added since.
    pub inputs: BTreeMap<String, String>,
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

/// The sha256 of every config file a derivation reads (a SKILL.md carries no
/// `schema_version` and is never read), by path.
pub(crate) fn derivation_inputs(files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, String> {
    files
        .iter()
        .filter(|(file, _)| canon::kind_of(file).is_some_and(|k| k != FileKind::Skill))
        .map(|(file, bytes)| (file.clone(), sha256_hex(bytes)))
        .collect()
}

/// The first file whose bytes in `now` differ from `inputs` (changed,
/// removed, or a config file added since), if any.
pub(crate) fn changed_input(
    inputs: &BTreeMap<String, String>,
    now: &BTreeMap<String, Vec<u8>>,
) -> Option<String> {
    let now = derivation_inputs(now);
    inputs
        .keys()
        .chain(now.keys())
        .find(|file| inputs.get(*file) != now.get(*file))
        .cloned()
}

/// How each older config file of the project (`files`: project-relative
/// path to bytes, under `root`) stands for migration, derived from the
/// whole project at once. Files that are not older (current, newer, a
/// SKILL.md, or not readable as a config file, which the loader reports)
/// are absent.
pub(crate) fn derive(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Assessed> {
    let mut out = BTreeMap::new();
    let mut project = ProjectTrees {
        files: BTreeMap::new(),
    };
    let mut befores: BTreeMap<String, (i64, &[u8])> = BTreeMap::new();
    let mut lowest = checks::SCHEMA_VERSION;
    let inputs = derivation_inputs(files);
    for (file, bytes) in files {
        let Some(kind) = canon::kind_of(file) else {
            continue;
        };
        // A SKILL.md frontmatter carries no schema_version (Agent Skills).
        if kind == FileKind::Skill {
            continue;
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue;
        };
        let Ok(doc) = canon::parse(kind, text) else {
            continue;
        };
        let Some(from) = version_of(&doc.tree) else {
            continue;
        };
        let mut stepping = false;
        if from < checks::SCHEMA_VERSION {
            if path_from(from).is_none() {
                out.insert(file.clone(), Assessed::NoPath(from));
            } else if let Some(link) = linked(root, file) {
                out.insert(file.clone(), Assessed::Blocked(Blocked::Link(link)));
            } else if !secret_lines(file, bytes).is_empty() {
                // A key-looking value would reach the diff and the stepped
                // bytes: the owner removes it by hand first.
                out.insert(file.clone(), Assessed::Blocked(Blocked::Secret));
            } else {
                stepping = true;
                lowest = lowest.min(from);
                befores.insert(file.clone(), (from, bytes.as_slice()));
            }
        }
        project.files.insert(
            file.clone(),
            ProjectFile {
                kind,
                doc,
                at: from,
                stepping,
                notes: Vec::new(),
                refused: None,
            },
        );
    }
    for from in lowest..checks::SCHEMA_VERSION {
        let Some(step) = STEPS.iter().find(|s| s.from == from) else {
            continue;
        };
        (step.change)(&mut project, from);
        let to = serde_json::Value::from(from + 1);
        for file in project.files.values_mut() {
            if !file.steps_at(from) {
                continue;
            }
            match canon::apply(
                &mut file.doc.tree,
                &["schema_version".to_string()],
                Some(&to),
            ) {
                Ok(()) => file.at = from + 1,
                Err(why) => file.refused = Some(why),
            }
        }
    }
    for (file, (from, bytes)) in befores {
        let Some(f) = project.files.remove(&file) else {
            continue;
        };
        if let Some(why) = f.refused {
            out.insert(file, Assessed::Blocked(Blocked::Step(why)));
            continue;
        }
        let after = match canon::emit(&f.doc) {
            Ok(text) => text.into_bytes(),
            Err(why) => {
                out.insert(file, Assessed::Blocked(Blocked::Step(why)));
                continue;
            }
        };
        out.insert(
            file.clone(),
            Assessed::Offer(MigrationOffer {
                diff: linediff::unified(&file, bytes, &after),
                file,
                from,
                to: checks::SCHEMA_VERSION,
                before_sha256: sha256_hex(bytes),
                after,
                notes: f.notes,
                inputs: inputs.clone(),
            }),
        );
    }
    out
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
    let waiting = reports
        .values()
        .any(|rep| rep.findings.iter().any(|f| f.fix == offered));
    if !waiting {
        return;
    }
    let derived = derive(root, files);
    for (file, rep) in reports.iter_mut() {
        if !rep.findings.iter().any(|f| f.fix == offered) {
            continue;
        }
        let fix = match derived.get(file) {
            Some(Assessed::Blocked(Blocked::Secret)) => checks::migration_blocked_fix(),
            Some(Assessed::Blocked(Blocked::Link(link))) => checks::migration_link_fix(file, link),
            Some(Assessed::Blocked(Blocked::Step(why))) => checks::migration_failed_fix(why),
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
    derive(root, files)
        .into_iter()
        .filter(|(file, _)| !accepted.contains_key(file))
        .filter_map(|(file, a)| match a {
            Assessed::Offer(_) => Some(format!(
                "{file} waits for its migration but is not in this accept; accept every offer at once"
            )),
            Assessed::Blocked(why) => Some(blocked_words(&file, &why)),
            Assessed::NoPath(_) => None,
        })
        .collect()
}

/// Words for each older file of `files` that has a step path but cannot be
/// offered (the session names them instead of asking for an accept).
pub(crate) fn blocked(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    derive(root, files)
        .into_iter()
        .filter_map(|(file, a)| match a {
            Assessed::Blocked(why) => Some(blocked_words(&file, &why)),
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
    derive(root, files)
        .into_iter()
        .filter_map(|(file, a)| match a {
            Assessed::NoPath(n) => Some(format!("{file} (schema_version {n})")),
            _ => None,
        })
        .collect()
}

/// Words for each older file of `files` that has no step path.
pub(crate) fn unmigratable(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> Vec<String> {
    derive(root, files)
        .into_iter()
        .filter_map(|(file, a)| match a {
            Assessed::NoPath(n) => Some(format!(
                "{file} is at schema_version {n}, which this app cannot migrate; rewrite it by hand"
            )),
            _ => None,
        })
        .collect()
}

/// The offers for a map of project-relative paths to bytes (the config
/// files among them, taken as the whole project), sorted by file.
pub(crate) fn offers_in(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> Vec<MigrationOffer> {
    derive(root, files)
        .into_values()
        .filter_map(|a| match a {
            Assessed::Offer(offer) => Some(offer),
            _ => None,
        })
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

/// Refuses the offers unless each is exactly the app's migration of the
/// whole project's current bytes `project` (same versions, stepped bytes
/// and diff), re-derived here: accept writes only what the registered steps
/// produce from what is on disk now, never other content.
pub(crate) fn check_offers(
    root: &Path,
    project: &BTreeMap<String, Vec<u8>>,
    offers: &[MigrationOffer],
) -> Result<(), WriteError> {
    let derived = derive(root, project);
    for offer in offers {
        match derived.get(&offer.file) {
            Some(Assessed::Offer(own))
                if own.from == offer.from
                    && own.to == offer.to
                    && own.after == offer.after
                    && own.diff == offer.diff => {}
            _ => {
                return Err(WriteError::BadEdit(format!(
                    "{}: the offer does not match the app's migration of this file; ask for the migration offers again",
                    offer.file
                )));
            }
        }
    }
    Ok(())
}
