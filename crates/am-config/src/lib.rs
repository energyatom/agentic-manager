//! Project config loader and validator (AD-16, AD-35, epic R1). Only this
//! crate reads project files. [`load`] reads every file kind of the project
//! layout, checks it and returns the findings, plus an immutable
//! [`ConfigSnapshot`] keyed by content hash when there is no error.
//!
//! Files: `company.yaml`, `models/{accounts,prices,capabilities}.yaml`,
//! `tools.yaml`, `seats/<id>.md` (YAML frontmatter plus a Markdown body),
//! `workflows/`, `tables/`, `dashboards/` and `sources/` as `<id>.yaml`, and
//! `schemas/<id>.json`, and `skills/<id>/` folders (SKILL.md plus
//! `references/`). YAML is read with serde-saphyr. Every file carries
//! `schema_version: 2` (story 1.14); ids are kebab-case file stems; references are checked
//! across files; secret-looking values are errors; YAML comments are warnings
//! because the app's writer does not keep them. A seat's capabilities are
//! expanded into its grants by the fixed table in `am_domain::capability`
//! (AD-46); a skill never adds a grant (AD-38).
//!
//! [`ConfigWriter`] is the only code that writes these files: canonical key
//! order, Markdown bodies kept byte for byte, atomic replace, History through
//! the `am_domain::ConfigHistory` port, Undo and Restore (story 1.8), and the
//! targeted `project_id` insert of [`ConfigIdentityAssign`] (story 1.13).
//!
//! Story 1.10 (R4, AD-7): [`migration_offers`] offers every config file at an
//! older `schema_version` as a diff and writes nothing; a project with an
//! offer builds no session ([`SessionError::MigrationPending`]); only
//! [`accept_migrations`] writes them, through the writer, as `migration`
//! History entries. Story 1.14 ships the first step, 1→2: a whole-project
//! step that moves `tools.yaml` `egress_hosts` into the fetching seats'
//! `allowed_hosts` (AD-48).
//!
//! Story 1.14 (AD-30, AD-48, Globs): a seat's `allowed_hosts` are parsed and
//! matched only by `am_domain::HostPattern`; linked folders follow the
//! amended root policy ([`LoadOptions`] names the home folder and the recent
//! project roots); every glob compiles through `am-glob`.
//!
//! Story 1.13 (AD-32): [`load_in`] names where reference files are checked
//! ([`LoadOptions`]: the project's runtime folder, a template's
//! `runtime-seed/`, or nowhere) and whether the folder is a template (which
//! carries no `project_id`); [`read_company`] and [`decisions_locked`] are the
//! resolver's reads before a project opens.
//!
//! Story 2.1 (AD-15, AD-16): [`ConfigWriter::create`] writes a new config
//! file (a [`ConfigCreate`]) without ever replacing one, and the reads and
//! patches of the interim CLI Account edit are here too ([`accounts_file`],
//! [`account_uses`], [`add_account_edit`], [`remove_account_edit`],
//! [`new_accounts_file`], [`outside_changes`]).
//!
//! Story 2.3: [`compatible_base_url`] is the one rule for an
//! `openai-compatible` Account's `base_url`, applied by the loader and by
//! `agentic accounts`; [`AccountsFile::entry_errors`] names the errors of
//! one entry of this Mac's user Accounts file.
//!
//! Story 1.9 (R2, AD-16): [`ConfigSession`] holds the accepted bytes of every
//! file the snapshot is read from and builds the snapshot only from them (an
//! exact-map load, no disk read of those files); a `notify` watcher announces
//! each outside change once as a [`ConfigEvent`], and
//! [`ConfigSession::reload`] applies every pending change as one new snapshot,
//! recording each file in History as an `outside_edit`.
#![forbid(unsafe_code)]

mod account_edit;
mod base_url;
mod canon;
mod checks;
mod cron;
pub mod entities;
mod fields;
mod finding;
mod kinds;
mod linediff;
mod migrate;
mod node;
mod refs;
mod schema_check;
mod session;
mod skills;
mod snapshot;
pub mod user_accounts;
mod writer;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use account_edit::{
    AccountUse, AccountsFile, account_uses, accounts_file, add_account_edit, is_config_id,
    new_accounts_file, outside_changes, remove_account_edit, reserved_account_ids,
};
pub use base_url::{COMPATIBLE_BASE_URL_FORMS, CompatibleBaseUrl, compatible_base_url};
pub use entities::*;
pub use finding::{Finding, Severity};
pub use migrate::{MigrationOffer, accept_migrations, migration_offers};
pub use session::{ConfigEvent, ConfigSession, ReloadError, SessionError};
pub use snapshot::ConfigSnapshot;
pub use user_accounts::{LEGACY_ACCOUNTS_FILE, USER_ACCOUNTS_FILE, UserAccounts, key_ref};
#[cfg(feature = "test-support")]
pub use writer::FailPoint;
pub use writer::{
    ConfigCreate, ConfigEdit, ConfigIdentityAssign, ConfigWriter, EditOp, ReconcileReport,
    WriteError, canonical_bytes, config_files,
};

use finding::Report;
use refs::{Known, Refs};

/// Why a project could not be read at all (individual file problems are
/// [`Finding`]s instead).
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("no project folder at {0}")]
    NotFound(PathBuf),
    #[error("{0} is not a folder")]
    NotADirectory(PathBuf),
}

/// The result of reading a project.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// Every problem, sorted by file, then line.
    pub findings: Vec<Finding>,
    /// Present only when no finding is an error.
    pub snapshot: Option<ConfigSnapshot>,
}

impl Loaded {
    pub fn error_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .count()
    }
    pub fn warning_count(&self) -> usize {
        self.findings.len() - self.error_count()
    }
}

/// The per-entity folders: (folder, extension).
const FOLDERS: &[(&str, &str)] = &[
    ("seats", "md"),
    ("workflows", "yaml"),
    ("tables", "yaml"),
    ("schemas", "json"),
    ("dashboards", "yaml"),
    ("sources", "yaml"),
];
const MODEL_FILES: &[&str] = &["accounts", "prices", "capabilities"];

/// How [`load_in`] reads a folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadOptions {
    /// Where seats' reference files are looked for (`<folder>/<seat-id>/`);
    /// `None` skips the existence warning.
    pub knowledge_root: Option<PathBuf>,
    /// The folder is a template or fixture: a `project_id` is an error.
    pub template: bool,
    /// The owner's home folder, for `~/` paths and the `~/Library` rules
    /// (AD-30); `None` reads `HOME` (tests inject a folder).
    pub home: Option<PathBuf>,
    /// The roots of the other recent projects (AD-30): a linked folder that
    /// is, holds or lies inside one is refused. The caller passes them (this
    /// crate keeps no index); `agentic validate` passes none.
    pub recent_roots: Vec<PathBuf>,
    /// This Mac's Accounts (story 2.12, AD-17): a project names them by id
    /// only. With a list, an id it lacks is a Warning at each line naming
    /// it; `None` (a template, a library test) checks an Account reference
    /// for form only. The reserved local ids always resolve.
    pub user_accounts: Option<UserAccounts>,
}

impl LoadOptions {
    /// A project, checked against `knowledge_root` (its runtime folder's
    /// `knowledge/`, when it has one on this Mac).
    pub fn project(knowledge_root: Option<PathBuf>) -> LoadOptions {
        LoadOptions {
            knowledge_root,
            template: false,
            ..LoadOptions::default()
        }
    }

    /// A template or fixture at `root`: reference files under its
    /// `runtime-seed/knowledge/`, and no `project_id`.
    pub fn template(root: &Path) -> LoadOptions {
        LoadOptions {
            knowledge_root: Some(am_domain::runtime::seed_knowledge(root)),
            template: true,
            ..LoadOptions::default()
        }
    }
}

/// Reads and checks the project at `root` (no reference-file check).
pub fn load(root: &Path) -> Result<Loaded, LoadError> {
    load_in(root, LoadOptions::default())
}

/// Reads and checks the folder at `root` as `opts` says.
pub fn load_in(root: &Path, opts: LoadOptions) -> Result<Loaded, LoadError> {
    load_full(root, BTreeMap::new(), opts)
}

/// [`load`], reading each file named in `overrides` (project-relative path
/// to bytes) from there instead of from disk, so the writer checks an edit
/// before anything is written. Only existing files are overridden: the
/// folders are still listed from disk.
pub fn load_with(root: &Path, overrides: BTreeMap<String, Vec<u8>>) -> Result<Loaded, LoadError> {
    load_full(root, overrides, LoadOptions::default())
}

/// [`load_in`] from an exact file map (story 1.9): every tracked file (see
/// [`tracked`]) exists only when `files` names it and is read only from
/// there, never from disk, so a snapshot holds no bytes the owner has not
/// accepted. Folders, file metadata and untracked paths still come from disk.
pub(crate) fn load_exact(
    root: &Path,
    files: BTreeMap<String, Vec<u8>>,
    opts: LoadOptions,
) -> Result<Loaded, LoadError> {
    load_inner(root, BTreeMap::new(), Some(files), opts)
}

fn load_full(
    root: &Path,
    overrides: BTreeMap<String, Vec<u8>>,
    opts: LoadOptions,
) -> Result<Loaded, LoadError> {
    load_inner(root, overrides, None, opts)
}

fn load_inner(
    root: &Path,
    overrides: BTreeMap<String, Vec<u8>>,
    exact: Option<BTreeMap<String, Vec<u8>>>,
    opts: LoadOptions,
) -> Result<Loaded, LoadError> {
    if !root.exists() {
        return Err(LoadError::NotFound(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(LoadError::NotADirectory(root.to_path_buf()));
    }
    let mut loader = Loader {
        places: kinds::Places {
            project: root.to_path_buf(),
            home: opts.home.or_else(home_dir),
            knowledge_root: opts.knowledge_root,
            recent_roots: opts.recent_roots,
        },
        template: opts.template,
        user_accounts: opts.user_accounts,
        overrides,
        exact,
        ..Loader::default()
    };
    loader.read_all(root);
    Ok(loader.finish())
}

/// The owner's home folder from `HOME`, when set.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// True when the loader reads the bytes of `rel` (forward slashes, relative)
/// into the snapshot: a config file the writer edits, or a file in a skill's
/// `references/` folder. The session tracks exactly these paths.
pub(crate) fn tracked(rel: &str) -> bool {
    if canon::kind_of(rel).is_some() {
        return true;
    }
    let parts: Vec<&str> = rel.split('/').collect();
    let fine = |p: &&str| !p.is_empty() && *p != "." && *p != ".." && !p.contains(['\\', '\0']);
    matches!(parts.as_slice(), ["skills", id, "references", name]
        if parts.iter().all(fine) &&!id.starts_with('.') && *name != ".DS_Store")
}

/// One folder entry as the loader sees it (from disk, or from an exact map).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Entry {
    pub link: bool,
    pub dir: bool,
    pub file: bool,
    pub exec: bool,
    pub len: u64,
}

impl Entry {
    fn of(meta: &std::fs::Metadata) -> Entry {
        Entry {
            link: meta.file_type().is_symlink(),
            dir: meta.is_dir(),
            file: meta.is_file(),
            exec: executable(meta),
            len: meta.len(),
        }
    }
}

#[cfg(unix)]
fn executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.is_file() && meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_meta: &std::fs::Metadata) -> bool {
    false
}

/// `dir/name`, or `name` at the root.
fn join_rel(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

#[derive(Default)]
struct Loader {
    places: kinds::Places,
    /// A template or fixture (story 1.13): no `project_id`.
    template: bool,
    /// Bytes to read instead of a file's (see [`load_with`]).
    overrides: BTreeMap<String, Vec<u8>>,
    /// The exact map of tracked files (see [`load_exact`]).
    exact: Option<BTreeMap<String, Vec<u8>>>,
    reports: BTreeMap<String, Report>,
    files: BTreeMap<String, Vec<u8>>,
    refs: Refs,
    /// Ids as declared, whether or not their entity passed every check.
    declared: Known,
    company: Option<Company>,
    /// This Mac's Accounts, when the caller passed them (story 2.12).
    user_accounts: Option<UserAccounts>,
    prices: Prices,
    capabilities: Capabilities,
    tools: ToolsConfig,
    seats: BTreeMap<String, Seat>,
    workflows: BTreeMap<String, Workflow>,
    tables: BTreeMap<String, Table>,
    schemas: BTreeMap<String, serde_json::Value>,
    dashboards: BTreeMap<String, Dashboard>,
    sources: BTreeMap<String, Source>,
    skills: BTreeMap<String, Skill>,
}

impl Loader {
    fn report(&mut self, rel: &str) -> &mut Report {
        self.reports
            .entry(rel.to_string())
            .or_insert_with(|| Report::new(rel))
    }

    /// The file's text, recorded for the hash; `None` (with a finding) when
    /// it cannot be read as UTF-8.
    fn read(&mut self, root: &Path, rel: &str) -> Option<String> {
        let read = match (self.overrides.get(rel), &self.exact) {
            (Some(b), _) => Ok(b.clone()),
            (None, Some(map)) if tracked(rel) => map
                .get(rel)
                .cloned()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound)),
            (None, _) => std::fs::read(root.join(rel)),
        };
        let bytes = match read {
            Ok(b) => b,
            Err(e) => {
                self.report(rel).error(
                    1,
                    format!("cannot read the file: {e}"),
                    "check that the file exists and can be read",
                );
                return None;
            }
        };
        self.files.insert(rel.to_string(), bytes.clone());
        match String::from_utf8(bytes) {
            Ok(text) => Some(text),
            Err(_) => {
                self.report(rel)
                    .error(1, "the file is not UTF-8 text", "save the file as UTF-8");
                None
            }
        }
    }

    /// True when the file `rel` exists (a link is followed).
    fn exists(&self, root: &Path, rel: &str) -> bool {
        match &self.exact {
            Some(map) if tracked(rel) => map.contains_key(rel),
            _ => root.join(rel).is_file(),
        }
    }

    /// The entries of the folder `dir` (project-relative, `""` for the root)
    /// with their metadata, links not followed, sorted by name; nothing when
    /// it is absent. Only `.DS_Store` (Finder's own file) is skipped. With an
    /// exact map, a tracked file is listed only when the map names it.
    pub(crate) fn listing(&self, root: &Path, dir: &str) -> Vec<(String, Entry)> {
        let mut out: BTreeMap<String, Entry> = BTreeMap::new();
        if let Ok(read) = std::fs::read_dir(root.join(dir)) {
            for e in read.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name == ".DS_Store" {
                    continue;
                }
                if let Ok(meta) = std::fs::symlink_metadata(e.path()) {
                    out.insert(name, Entry::of(&meta));
                }
            }
        }
        if let Some(map) = &self.exact {
            // A tracked regular file exists only when the map names it; any
            // other entry (a folder, a link) stays, so its checks still run.
            out.retain(|name, e| !e.file || !tracked(&join_rel(dir, name)));
            let prefix = if dir.is_empty() {
                String::new()
            } else {
                format!("{dir}/")
            };
            for (rel, bytes) in map.range(prefix.clone()..) {
                let Some(rest) = rel.strip_prefix(&prefix) else {
                    break;
                };
                match rest.split_once('/') {
                    None => {
                        let on_disk = std::fs::symlink_metadata(root.join(rel)).ok();
                        let exec = on_disk.as_ref().is_some_and(executable);
                        out.insert(
                            rest.to_string(),
                            Entry {
                                file: true,
                                exec,
                                len: bytes.len() as u64,
                                ..Entry::default()
                            },
                        );
                    }
                    Some((sub, _)) => {
                        out.entry(sub.to_string()).or_insert(Entry {
                            dir: true,
                            ..Entry::default()
                        });
                    }
                }
            }
        }
        out.into_iter().collect()
    }

    /// The non-hidden file names in the folder `dir`, sorted.
    fn entries(&self, root: &Path, dir: &str) -> Vec<String> {
        self.listing(root, dir)
            .into_iter()
            .filter(|(name, e)| e.file && !name.starts_with('.'))
            .map(|(name, _)| name)
            .collect()
    }

    fn read_all(&mut self, root: &Path) {
        // company.yaml is required; the rest are optional.
        if self.exists(root, "company.yaml") {
            if let Some(text) = self.read(root, "company.yaml") {
                let mut rep = Report::new("company.yaml");
                if let Some(node) = kinds::yaml(&mut rep, &text, 0) {
                    self.declared
                        .teams
                        .extend(kinds::declared_ids(&node, "teams"));
                    self.company = kinds::company(&mut rep, &mut self.refs, &node, self.template);
                }
                self.merge(rep);
            }
        } else {
            self.report("company.yaml").error(
                1,
                "missing company.yaml",
                format!(
                    "create company.yaml with schema_version: {}, name, timezone, currency and teams",
                    checks::SCHEMA_VERSION
                ),
            );
        }
        if self.exists(root, "tools.yaml")
            && let Some(text) = self.read(root, "tools.yaml")
        {
            let mut rep = Report::new("tools.yaml");
            if let Some(node) = kinds::yaml(&mut rep, &text, 0) {
                self.declared
                    .mcp_tools
                    .extend(kinds::declared_mcp_tools(&node));
                self.tools = kinds::tools(&mut rep, &mut self.refs, &node);
            }
            self.merge(rep);
        }
        for stem in MODEL_FILES {
            let rel = format!("models/{stem}.yaml");
            if !self.exists(root, &rel) {
                continue;
            }
            if *stem == "accounts" {
                // Story 2.12 (AD-17): Accounts belong to this Mac. The old
                // per-project file is an error and is never parsed, so
                // nothing in it declares an Account or reaches a key.
                let project = self.places.project.display().to_string();
                self.report(&rel).error(
                    1,
                    format!(
                        "{LEGACY_ACCOUNTS_FILE} holds Accounts, which now live on this Mac (one list for every project), not in a project; this file is not read"
                    ),
                    format!(
                        "move them to this Mac's Accounts with `agentic accounts migrate --project {project}`, which removes this file"
                    ),
                );
                continue;
            }
            let Some(text) = self.read(root, &rel) else {
                continue;
            };
            let mut rep = Report::new(&rel);
            if let Some(node) = kinds::yaml(&mut rep, &text, 0) {
                match *stem {
                    "prices" => self.prices = kinds::prices(&mut rep, &node),
                    _ => self.capabilities = kinds::capabilities(&mut rep, &node),
                }
            }
            self.merge(rep);
        }
        for name in self.entries(root, "models") {
            let known = MODEL_FILES.iter().any(|m| name == format!("{m}.yaml"));
            if !known {
                self.report(&format!("models/{name}")).warning(
                    1,
                    "this file is not read",
                    "models/ holds accounts.yaml, prices.yaml and capabilities.yaml only",
                );
            }
        }
        skills::load_all(self, root);
        for (folder, ext) in FOLDERS {
            for name in self.entries(root, folder) {
                let rel = format!("{folder}/{name}");
                let Some(stem) = name.strip_suffix(&format!(".{ext}")) else {
                    self.report(&rel).warning(
                        1,
                        "this file is not read",
                        format!("{folder}/ holds <id>.{ext} files only; rename or move it"),
                    );
                    continue;
                };
                let stem = stem.to_string();
                if !checks::is_kebab(&stem) {
                    self.report(&rel).error(
                        1,
                        format!("file name '{stem}' is not a kebab-case id"),
                        format!(
                            "rename the file to {}.{ext} (lowercase words joined by hyphens; the file name is the id)",
                            checks::kebab_of(&stem)
                        ),
                    );
                }
                match *folder {
                    "seats" => {
                        self.declared.seats.insert(stem.clone());
                    }
                    "schemas" => {
                        self.declared.schemas.insert(stem.clone());
                    }
                    "workflows" => {
                        self.declared.workflows.insert(stem.clone(), (None, None));
                    }
                    "tables" => {
                        self.declared.tables.insert(stem.clone(), None);
                    }
                    _ => {}
                }
                let Some(text) = self.read(root, &rel) else {
                    continue;
                };
                self.entity(folder, &rel, &stem, &text);
            }
        }
    }

    fn entity(&mut self, folder: &str, rel: &str, stem: &str, text: &str) {
        let mut rep = Report::new(rel);
        let refs = &mut self.refs;
        match folder {
            "seats" => {
                if let Some((front, offset, body)) = frontmatter(&mut rep, text, &seat_fix())
                    && let Some(node) = kinds::yaml(&mut rep, front, offset)
                {
                    checks::text_secrets(&mut rep, body, body_first_line(text, body));
                    if kinds::declared_crawl_on(&node) {
                        self.declared.crawl_seats.insert(stem.to_string());
                    }
                    self.declared
                        .seat_hosts
                        .insert(stem.to_string(), kinds::declared_hosts(&node));
                    if let Some(seat) = kinds::seat(&mut rep, refs, &self.places, stem, &node, body)
                    {
                        self.seats.insert(stem.to_string(), seat);
                    }
                }
            }
            "schemas" => {
                if let Some(value) = kinds::schema(&mut rep, text) {
                    self.schemas.insert(stem.to_string(), value);
                }
            }
            _ => {
                if let Some(node) = kinds::yaml(&mut rep, text, 0) {
                    if folder == "tables" {
                        self.declared.tables.insert(
                            stem.to_string(),
                            Some(kinds::declared_columns(&node).into_iter().collect()),
                        );
                    }
                    if folder == "workflows" {
                        let (team, steps) = kinds::declared_workflow(&node);
                        self.declared
                            .workflows
                            .insert(stem.to_string(), (team, Some(steps.into_iter().collect())));
                    }
                    match folder {
                        "workflows" => {
                            if let Some(w) = kinds::workflow(&mut rep, refs, stem, &node) {
                                self.workflows.insert(stem.to_string(), w);
                            }
                        }
                        "tables" => {
                            if let Some(t) = kinds::table(&mut rep, refs, stem, &node) {
                                self.tables.insert(stem.to_string(), t);
                            }
                        }
                        "dashboards" => {
                            if let Some(d) = kinds::dashboard(&mut rep, refs, stem, &node) {
                                self.dashboards.insert(stem.to_string(), d);
                            }
                        }
                        _ => {
                            if let Some(s) = kinds::source(&mut rep, refs, stem, &node, text) {
                                self.sources.insert(stem.to_string(), s);
                            }
                        }
                    }
                }
            }
        }
        self.merge(rep);
    }

    fn merge(&mut self, rep: Report) {
        let file = rep.file.clone();
        self.report(&file).findings.extend(rep.findings);
    }

    fn finish(mut self) -> Loaded {
        let mut known = std::mem::take(&mut self.declared);
        known.builtin_tools = am_domain::tool_manifest::builtin_tool_ids();
        if let Some(list) = &self.user_accounts {
            known.user_list = true;
            for a in list.accounts() {
                known.accounts.insert(a.id.clone());
                known
                    .account_providers
                    .insert(a.id.clone(), a.provider.clone());
            }
        }
        refs::resolve(&self.refs, &known, &mut self.reports);
        migrate::word_blocked(&self.places.project, &mut self.reports, &self.files);
        let mut findings: Vec<Finding> = self
            .reports
            .into_values()
            .flat_map(|r| r.findings)
            .collect();
        findings.sort_by(|a, b| {
            (&a.file, a.line, a.severity, &a.message)
                .cmp(&(&b.file, b.line, b.severity, &b.message))
        });
        findings.dedup();
        let has_error = findings.iter().any(|f| f.severity == Severity::Error);
        let snapshot = match (has_error, self.company) {
            (false, Some(company)) => Some(ConfigSnapshot {
                hash: snapshot::content_hash(&self.files),
                company,
                prices: self.prices,
                capabilities: self.capabilities,
                tools: self.tools,
                seats: self.seats,
                workflows: self.workflows,
                tables: self.tables,
                schemas: self.schemas,
                dashboards: self.dashboards,
                sources: self.sources,
                skills: self.skills,
            }),
            _ => None,
        };
        Loaded { findings, snapshot }
    }
}

fn seat_fix() -> String {
    format!(
        "start the file with a line `---`, the YAML fields (schema_version: {}, name, role, team), another `---`, then the instructions",
        checks::SCHEMA_VERSION
    )
}

/// Splits a seat file or a SKILL.md into (frontmatter, its line offset,
/// body). The file must start with a `---` line and the frontmatter ends at
/// the next `---`; `fix` says how to write it.
fn frontmatter<'t>(rep: &mut Report, text: &'t str, fix: &str) -> Option<(&'t str, u32, &'t str)> {
    let first_end = text.find('\n').map_or(text.len(), |i| i + 1);
    if text[..first_end].trim_end() != "---" {
        rep.error(1, "the file must start with YAML frontmatter", fix);
        return None;
    }
    let mut pos = first_end;
    while pos < text.len() {
        let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i + 1);
        if text[pos..end].trim_end() == "---" {
            return Some((&text[first_end..pos], 1, &text[end..]));
        }
        pos = end;
    }
    rep.error(1, "the frontmatter has no closing `---` line", fix);
    None
}

/// The file line on which `body` (a suffix of `text`) starts.
fn body_first_line(text: &str, body: &str) -> u32 {
    let start = text.len() - body.len();
    u32::try_from(text[..start].matches('\n').count()).unwrap_or(0) + 1
}

// ---------------------------------------------------------------- opening

/// What [`read_company`] found in `company.yaml` before a project opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompanyHead {
    pub project_id: Option<am_domain::ProjectId>,
    /// The line holding `project_id`, when there is one.
    pub project_id_line: Option<u32>,
}

/// Why a folder cannot be opened as a project, in words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompanyRefusal {
    #[error(
        "the folder holds no project yet; create one with the first-run wizard (opening the app in your browser), not from the command line"
    )]
    Empty,
    #[error(
        "the folder holds config files but no company.yaml, so it is not a project the app can open"
    )]
    NoCompanyYaml,
    #[error("company.yaml cannot be read: {0}")]
    Unreadable(String),
    #[error(
        "company.yaml has schema_version {found}, newer than this app knows ({known}); update the app"
    )]
    TooNew { found: i64, known: i64 },
    #[error(
        "company.yaml line {line}: project_id '{value}' is not a project id; write back the id this project had (its runtime folder's name), or remove the line to treat the folder as a new project"
    )]
    BadProjectId { line: u32, value: String },
}

/// True when `root` holds any config file or folder of a project.
fn has_config(root: &Path) -> bool {
    let files = ["company.yaml", "tools.yaml"];
    let dirs = ["models", "skills"];
    files.iter().any(|f| root.join(f).is_file())
        || dirs.iter().any(|d| root.join(d).is_dir())
        || FOLDERS.iter().any(|(d, _)| root.join(d).is_dir())
}

/// Reads `company.yaml`'s `schema_version` and `project_id` (nothing else is
/// checked: the open does not validate the project). Refused in words when
/// the folder holds no project, config files but no `company.yaml`, an
/// unreadable file, a newer `schema_version` or a malformed `project_id`.
pub fn read_company(root: &Path) -> Result<CompanyHead, CompanyRefusal> {
    let path = root.join("company.yaml");
    if !path.is_file() {
        return Err(if has_config(root) {
            CompanyRefusal::NoCompanyYaml
        } else {
            CompanyRefusal::Empty
        });
    }
    let bytes = std::fs::read(&path).map_err(|e| CompanyRefusal::Unreadable(e.to_string()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| CompanyRefusal::Unreadable("it is not UTF-8 text".into()))?;
    company_head(&text)
}

/// [`read_company`] on the text of `company.yaml`.
pub(crate) fn company_head(text: &str) -> Result<CompanyHead, CompanyRefusal> {
    let node = node::parse(text, 0)
        .map_err(|e| CompanyRefusal::Unreadable(format!("line {}: {}", e.line, e.message)))?;
    let entries = match &node.kind {
        node::Kind::Map(m) => m.as_slice(),
        node::Kind::Null => &[],
        _ => {
            return Err(CompanyRefusal::Unreadable(
                "it does not hold key: value pairs".into(),
            ));
        }
    };
    let get = |k: &str| {
        entries
            .iter()
            .find(|(key, _)| key.name == k)
            .map(|(_, v)| v)
    };
    if let Some(v) = get("schema_version")
        && let node::Kind::Int(n) = v.kind
        && n > checks::SCHEMA_VERSION
    {
        return Err(CompanyRefusal::TooNew {
            found: n,
            known: checks::SCHEMA_VERSION,
        });
    }
    let Some(v) = get("project_id") else {
        return Ok(CompanyHead {
            project_id: None,
            project_id_line: None,
        });
    };
    let value = match &v.kind {
        node::Kind::Str(s) => s.clone(),
        _ => v.raw.clone().unwrap_or_else(|| v.type_name().to_string()),
    };
    match am_domain::ProjectId::parse(&value) {
        Some(id) => Ok(CompanyHead {
            project_id: Some(id),
            project_id_line: Some(v.line),
        }),
        None => Err(CompanyRefusal::BadProjectId {
            line: v.line,
            value,
        }),
    }
}

/// The children that run as the owner which a project's config enables
/// (AD-13, AD-15), and the files that could not be read to tell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnerChildren {
    /// One reason per enabled child, in words: a stdio MCP server, a Source
    /// with `render: true`.
    pub enabled: Vec<String>,
    /// `tools.yaml` and Source files with an error: each is read as
    /// enabling a child (fails closed), though nothing says it does.
    pub broken: Vec<String>,
}

impl OwnerChildren {
    /// Every reason in words, joined; `None` when there is none.
    pub fn words(&self) -> Option<String> {
        let mut reasons = self.enabled.clone();
        reasons.extend(self.broken.iter().map(|file| {
            format!("{file} has an error, so it is read as enabling a child that runs as you")
        }));
        (!reasons.is_empty()).then(|| reasons.join("; "))
    }
}

pub(crate) fn stdio_server_words(id: &str) -> String {
    format!("the stdio MCP server `{id}` is enabled")
}

pub(crate) fn rendering_source_words(id: &str) -> String {
    format!("the Source `{id}` has render: true")
}

/// What the project's files at `root` enable now (see [`OwnerChildren`]).
/// Read through am-config with no store.
pub fn owner_children(root: &Path) -> OwnerChildren {
    let mut loader = Loader {
        places: kinds::Places {
            project: root.to_path_buf(),
            home: home_dir(),
            knowledge_root: None,
            recent_roots: Vec::new(),
        },
        ..Loader::default()
    };
    loader.read_all(root);
    let mut found = OwnerChildren::default();
    for s in &loader.tools.mcp_servers {
        if s.transport == "stdio" {
            found.enabled.push(stdio_server_words(&s.id));
        }
    }
    for s in loader.sources.values() {
        if s.render {
            found.enabled.push(rendering_source_words(&s.id));
        }
    }
    for (file, rep) in &loader.reports {
        let config = file == "tools.yaml" || file.starts_with("sources/");
        if config && rep.findings.iter().any(|f| f.severity == Severity::Error) {
            found.broken.push(file.clone());
        }
    }
    found
}

/// Why the CLI's project-open decisions (`--take-over`, `--confirm-move`)
/// move to the UI (AD-15): the project's config enables a stdio MCP server
/// or a Source with `render: true` (children that run as the owner). Read
/// through am-config with no store; a `tools.yaml` or Source file with an
/// error counts as enabling one (fails closed). `None` when neither.
/// [`owner_children`] gives the same reasons apart.
pub fn decisions_locked(root: &Path) -> Option<String> {
    owner_children(root).words()
}
