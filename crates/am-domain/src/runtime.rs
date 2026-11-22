//! The runtime folder (AD-32): a project's runtime state lives in
//! `<runtime base>/<project-id>/`, outside the project and outside every
//! cloud-synced folder. [`RuntimeLayout`] names every path inside it, and
//! this module is the only place in `crates/` that spells a runtime path
//! (CI fails on the literal elsewhere: `cargo xtask runtime-paths`).
//!
//! The runtime base itself is resolved by `am-app`'s platform adapter; this
//! module only names what lies inside it. It also holds the
//! [`ProjectIdentity`] port (AD-47): a root's volume and file id and the
//! Mac's name, read from the OS by `am-app` and faked by `am-testkit`.

use std::fmt;
use std::path::{Path, PathBuf};

use uuid::Uuid;

/// The in-project runtime folder older binaries wrote (`<project>/.agentic/`).
/// It is moved out on the first open by this binary and never written again.
pub const LEGACY_DIR: &str = ".agentic";

/// The one runtime file in a project folder: which Mac has it open.
pub const OPEN_MARKER: &str = ".agentic-open";

/// The kept copy of a moved legacy folder: `legacy-agentic-<YYYY-MM-DD>[-n]`.
pub const LEGACY_COPY_PREFIX: &str = "legacy-agentic-";

/// Where a template or fixture ships its seed reference files.
pub const RUNTIME_SEED: &str = "runtime-seed";

/// The runtime base's folder name inside the OS app-data folder.
pub const RUNTIME_BASE_NAME: &str = "agentic-manager-projects";

/// The app data dir's folder name inside the OS app-data folder.
pub const APP_DATA_NAME: &str = "agentic-manager";

/// Every path in the app data dir (AD-17, AD-32): this Mac's id, the folder
/// of running servers, the user Accounts file, the user store, its lock and
/// its pre-migration backups. Nothing else spells them (`cargo xtask
/// runtime-paths`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDataLayout {
    dir: PathBuf,
}

impl AppDataLayout {
    /// The layout of the app data dir at `dir` (already resolved).
    pub fn new(dir: PathBuf) -> AppDataLayout {
        AppDataLayout { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The user Accounts file (AD-17): only am-config reads and writes it.
    pub fn accounts_file(&self) -> PathBuf {
        self.dir.join("accounts.yaml")
    }

    /// The user store (AD-6, AD-17).
    pub fn user_db(&self) -> PathBuf {
        self.dir.join("user.db")
    }

    /// The user store's lock: a process writes the user store only while it
    /// holds it (AD-6).
    pub fn user_lock(&self) -> PathBuf {
        self.dir.join("user.lock")
    }

    /// `VACUUM INTO` copies of the user store taken before a migration; the
    /// three newest kept (AD-7).
    pub fn backups_pre_migration(&self) -> PathBuf {
        self.dir.join("backups").join("pre-migration")
    }

    /// This Mac's random id.
    pub fn mac_id(&self) -> PathBuf {
        self.dir.join("mac-id")
    }

    /// The folder where every running `agentic serve` records the port it
    /// listens on: one owner-only file per process, named by its process id
    /// (story 2.2, AD-12).
    pub fn serving_dir(&self) -> PathBuf {
        self.dir.join("serving")
    }
}

/// A project's id (UUIDv7), kept as `project_id` in `company.yaml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(Uuid);

impl ProjectId {
    /// A new id (UUIDv7).
    pub fn mint() -> ProjectId {
        ProjectId(Uuid::now_v7())
    }

    /// Parses the hyphenated lowercase form of a UUIDv7; anything else is
    /// `None`.
    pub fn parse(text: &str) -> Option<ProjectId> {
        let id = Uuid::try_parse(text).ok()?;
        (id.get_version_num() == 7 && id.hyphenated().to_string() == text).then_some(ProjectId(id))
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

/// This Mac's id: a random UUID minted once into the app data dir, never a
/// hardware id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacId(Uuid);

impl MacId {
    /// A new random id (UUIDv4).
    pub fn mint() -> MacId {
        MacId(Uuid::new_v4())
    }

    /// Parses a hyphenated lowercase UUID; anything else is `None`.
    pub fn parse(text: &str) -> Option<MacId> {
        let id = Uuid::try_parse(text).ok()?;
        (id.hyphenated().to_string() == text).then_some(MacId(id))
    }
}

impl fmt::Display for MacId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.hyphenated())
    }
}

/// `<project>/.agentic/`, the legacy in-project runtime folder.
pub fn legacy_dir(project_root: &Path) -> PathBuf {
    project_root.join(LEGACY_DIR)
}

/// `<project>/.agentic-open`, the open marker.
pub fn open_marker(project_root: &Path) -> PathBuf {
    project_root.join(OPEN_MARKER)
}

/// `<template>/runtime-seed/knowledge/`: a template's or fixture's seed
/// reference files (`<seat-id>/` and `team-<team-id>/` inside).
pub fn seed_knowledge(template_root: &Path) -> PathBuf {
    template_root.join(RUNTIME_SEED).join("knowledge")
}

/// True for a path component naming a legacy runtime folder (ignoring case,
/// as APFS does).
pub fn is_legacy_component(component: &str) -> bool {
    component.eq_ignore_ascii_case(LEGACY_DIR)
}

/// The runtime base (`agentic-manager-projects/`): one runtime folder per
/// project id, its lock and its root index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBase {
    dir: PathBuf,
}

impl RuntimeBase {
    pub fn new(dir: PathBuf) -> RuntimeBase {
        RuntimeBase { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `<base>/.lock`, held only while a project is resolved.
    pub fn lock(&self) -> PathBuf {
        self.dir.join(".lock")
    }

    /// `<base>/roots.index`: a cache from root identity to project id,
    /// rebuilt from the runtime folders' root records.
    pub fn root_index(&self) -> PathBuf {
        self.dir.join("roots.index")
    }

    /// The pre-open lock of a served folder with no open project, keyed by
    /// its root's identity (epic 5).
    pub fn pre_open_lock(&self, volume: &str, file_id: u64) -> PathBuf {
        self.dir.join(format!(".pre-open-{volume}-{file_id}.lock"))
    }

    /// The runtime folder of project `id`.
    pub fn project(&self, id: &ProjectId) -> RuntimeLayout {
        RuntimeLayout::at(self.dir.join(id.to_string()))
    }

    /// The project id a folder name in the base stands for, if it is one.
    pub fn project_of(name: &str) -> Option<ProjectId> {
        ProjectId::parse(name)
    }
}

/// Every path inside one runtime folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLayout {
    dir: PathBuf,
}

impl RuntimeLayout {
    /// The layout of the runtime folder at `dir` (already resolved).
    pub fn at(dir: PathBuf) -> RuntimeLayout {
        RuntimeLayout { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The exclusive project lock (holder record inside).
    pub fn lock(&self) -> PathBuf {
        self.dir.join("lock")
    }

    pub fn state_db(&self) -> PathBuf {
        self.dir.join("state.db")
    }

    /// The port `agentic serve` bound (owner-only, removed at exit): every
    /// egress client of the project refuses it to Accounts and MCP servers
    /// (AD-12, story 2.2).
    pub fn serve_port(&self) -> PathBuf {
        self.dir.join("serve.port")
    }

    /// `VACUUM INTO` copies taken before a migration; the three newest kept.
    pub fn backups_pre_migration(&self) -> PathBuf {
        self.dir.join("backups").join("pre-migration")
    }

    /// The daily encrypted backups (epic 9).
    pub fn backups_daily(&self) -> PathBuf {
        self.dir.join("backups").join("daily")
    }

    pub fn logs(&self) -> PathBuf {
        self.dir.join("logs")
    }

    pub fn inbox(&self) -> PathBuf {
        self.dir.join("inbox")
    }

    /// The drop folders.
    pub fn drops(&self) -> PathBuf {
        self.dir.join("drop")
    }

    pub fn records(&self) -> PathBuf {
        self.dir.join("records")
    }

    /// `knowledge/`, holding `<seat-id>/` and `team-<team-id>/`.
    pub fn knowledge_dir(&self) -> PathBuf {
        self.dir.join("knowledge")
    }

    /// A seat's reference files.
    pub fn knowledge(&self, seat: &str) -> PathBuf {
        self.knowledge_dir().join(seat)
    }

    /// A team's Knowledge notes.
    pub fn knowledge_team(&self, team: &str) -> PathBuf {
        self.knowledge_dir().join(format!("team-{team}"))
    }

    /// A seat's workspace.
    pub fn workspace(&self, seat: &str) -> PathBuf {
        self.dir.join("workspace").join(seat)
    }

    /// An MCP server's working folder (its child's cwd).
    pub fn mcp_workspace(&self, server: &str) -> PathBuf {
        self.dir.join("workspace").join("mcp").join(server)
    }

    /// The record of this folder's project root (authoritative; the base's
    /// root index is rebuilt from these).
    pub fn root_record(&self) -> PathBuf {
        self.dir.join("root")
    }

    /// The legacy-move journal, with the sha256 manifest.
    pub fn legacy_journal(&self) -> PathBuf {
        self.dir.join("legacy-move.journal")
    }

    /// The kept legacy folder for `date` (`YYYY-MM-DD`, UTC); `n` above 1
    /// adds `-n` (a name already taken).
    pub fn legacy_copy(&self, date: &str, n: u32) -> PathBuf {
        let name = if n <= 1 {
            format!("{LEGACY_COPY_PREFIX}{date}")
        } else {
            format!("{LEGACY_COPY_PREFIX}{date}-{n}")
        };
        self.dir.join(name)
    }

    /// True for a kept legacy folder's name: exempt from every sweep,
    /// retention and mode-repair walk (the mode check still covers it).
    pub fn is_kept_legacy(name: &str) -> bool {
        name.starts_with(LEGACY_COPY_PREFIX)
    }
}

// ---------------------------------------------------------------- identity

/// A project root's identity: its volume and its folder's file id. The path
/// is for display (and, off macOS, where the file id is looked up).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootIdentity {
    /// The volume's VolumeUUID (macOS) or its device number (elsewhere).
    pub volume: String,
    /// The root folder's file id (inode).
    pub file_id: u64,
    pub path: PathBuf,
}

impl RootIdentity {
    /// Same volume and file id (the path is not compared).
    pub fn same_root(&self, other: &RootIdentity) -> bool {
        self.volume == other.volume && self.file_id == other.file_id
    }
}

/// Where a recorded root stands now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeState {
    /// The recorded volume is not mounted.
    Unmounted,
    /// The volume is mounted. `root` is a path through which the recorded
    /// folder can be read when its file id still exists there, `None` when
    /// it is gone.
    Mounted { root: Option<PathBuf> },
}

/// Why a root's or the Mac's identity could not be read. No value is ever
/// guessed or filled in from a fallback.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    #[error(
        "the project is not opened: `{tool}` did not give the project's identity; it returned: {output}"
    )]
    Tool { tool: String, output: String },
    #[error(
        "the project folder {} is on a volume with no VolumeUUID, such as a network share; opening a project there is not supported in v1; move it to a disk on this Mac",
        .path.display()
    )]
    NetworkShare { path: PathBuf },
    #[error("reading a project's identity is not supported on this platform yet")]
    Unsupported,
}

/// The OS identity of project roots and of this Mac (AD-32, AD-47).
pub trait ProjectIdentity: Send + Sync {
    /// The identity of the folder at `root`.
    fn root(&self, root: &Path) -> Result<RootIdentity, IdentityError>;
    /// Where the recorded root stands now.
    fn locate(&self, recorded: &RootIdentity) -> Result<VolumeState, IdentityError>;
    /// This Mac's name (for display, and the store's lock record).
    fn mac_name(&self) -> Result<String, IdentityError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_parse_only_their_canonical_form() {
        let id = ProjectId::mint();
        assert_eq!(ProjectId::parse(&id.to_string()), Some(id));
        assert_eq!(ProjectId::parse(&id.to_string().to_uppercase()), None);
        assert_eq!(
            ProjectId::parse("6f1e2a3b-0000-4000-8000-000000000000"),
            None,
            "not a v7"
        );
        let mac = MacId::mint();
        assert_eq!(MacId::parse(&mac.to_string()), Some(mac));
        assert_eq!(MacId::parse("x"), None);
    }

    #[test]
    fn layout_names_and_the_kept_legacy_rule() {
        let l = RuntimeLayout::at(PathBuf::from("/b/p"));
        assert_eq!(
            l.legacy_copy("2026-11-21", 1),
            Path::new("/b/p/legacy-agentic-2026-11-21")
        );
        assert_eq!(
            l.legacy_copy("2026-11-21", 3),
            Path::new("/b/p/legacy-agentic-2026-11-21-3")
        );
        assert!(RuntimeLayout::is_kept_legacy("legacy-agentic-2026-11-21-2"));
        assert!(!RuntimeLayout::is_kept_legacy("backups"));
        assert_eq!(l.knowledge_team("t"), Path::new("/b/p/knowledge/team-t"));
        assert_eq!(legacy_dir(Path::new("/p")), Path::new("/p/.agentic"));
    }
}
