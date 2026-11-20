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
//! `schema_version: 1`; ids are kebab-case file stems; references are checked
//! across files; secret-looking values are errors; YAML comments are warnings
//! because the app's writer does not keep them. A seat's capabilities are
//! expanded into its grants by the fixed table in `am_domain::capability`
//! (AD-46); a skill never adds a grant (AD-38). The writer, History, the
//! watcher and config migrations come in later stories.
#![forbid(unsafe_code)]

mod checks;
mod cron;
pub mod entities;
mod fields;
mod finding;
mod kinds;
mod node;
mod refs;
mod schema_check;
mod skills;
mod snapshot;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use entities::*;
pub use finding::{Finding, Severity};
pub use snapshot::ConfigSnapshot;

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

/// Reads and checks the project at `root`.
pub fn load(root: &Path) -> Result<Loaded, LoadError> {
    if !root.exists() {
        return Err(LoadError::NotFound(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(LoadError::NotADirectory(root.to_path_buf()));
    }
    let mut loader = Loader {
        places: kinds::Places {
            project: root.to_path_buf(),
            home: std::env::var_os("HOME").map(PathBuf::from),
        },
        ..Loader::default()
    };
    loader.read_all(root);
    Ok(loader.finish())
}

#[derive(Default)]
struct Loader {
    places: kinds::Places,
    reports: BTreeMap<String, Report>,
    files: BTreeMap<String, Vec<u8>>,
    refs: Refs,
    /// Ids as declared, whether or not their entity passed every check.
    declared: Known,
    company: Option<Company>,
    accounts: Vec<Account>,
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
        let bytes = match std::fs::read(root.join(rel)) {
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

    fn read_all(&mut self, root: &Path) {
        // company.yaml is required; the rest are optional.
        if root.join("company.yaml").is_file() {
            if let Some(text) = self.read(root, "company.yaml") {
                let mut rep = Report::new("company.yaml");
                if let Some(node) = kinds::yaml(&mut rep, &text, 0) {
                    self.declared
                        .teams
                        .extend(kinds::declared_ids(&node, "teams"));
                    self.company = kinds::company(&mut rep, &mut self.refs, &node);
                }
                self.merge(rep);
            }
        } else {
            self.report("company.yaml").error(
                1,
                "missing company.yaml",
                "create company.yaml with schema_version: 1, name, timezone, currency and teams",
            );
        }
        if root.join("tools.yaml").is_file()
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
            if !root.join(&rel).is_file() {
                continue;
            }
            let Some(text) = self.read(root, &rel) else {
                continue;
            };
            let mut rep = Report::new(&rel);
            if let Some(node) = kinds::yaml(&mut rep, &text, 0) {
                match *stem {
                    "accounts" => {
                        self.declared
                            .accounts
                            .extend(kinds::declared_ids(&node, "accounts"));
                        self.declared
                            .account_providers
                            .extend(kinds::declared_account_providers(&node));
                        self.accounts = kinds::accounts(&mut rep, &node);
                    }
                    "prices" => self.prices = kinds::prices(&mut rep, &node),
                    _ => self.capabilities = kinds::capabilities(&mut rep, &node),
                }
            }
            self.merge(rep);
        }
        for name in entries(&root.join("models")) {
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
            for name in entries(&root.join(folder)) {
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
                if let Some((front, offset, body)) = frontmatter(&mut rep, text, SEAT_FIX)
                    && let Some(node) = kinds::yaml(&mut rep, front, offset)
                {
                    checks::text_secrets(&mut rep, body, body_first_line(text, body));
                    if kinds::declared_crawl_on(&node) {
                        self.declared.crawl_seats.insert(stem.to_string());
                    }
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
        known.egress_hosts = self.tools.egress_hosts.clone();
        refs::resolve(&self.refs, &known, &mut self.reports);
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
                accounts: self.accounts,
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

/// The non-hidden file names in `dir`, sorted; nothing when it is absent.
fn entries(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = read
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    names.sort();
    names
}

const SEAT_FIX: &str = "start the file with a line `---`, the YAML fields (schema_version: 1, name, role, team), another `---`, then the instructions";

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
