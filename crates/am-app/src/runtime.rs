//! Opening a project (AD-32, story 1.13): its runtime folder lives at
//! `<runtime base>/<project-id>/`, outside the project; one Mac opens it at
//! a time, through the `.agentic-open` marker.
//!
//! am-app alone resolves the runtime base, the app data dir and this Mac's
//! id ([`Places`], [`read_mac_id`]); it writes each runtime folder's root
//! record and the base's root index (record first, each by temp file and
//! rename; the record is authoritative and the index a cache rebuilt from
//! the records). [`open_project`] follows AD-32's check order; every check
//! reads before anything is written, and the first that fails decides the
//! words:
//!
//! 1. the identity tools (the [`ProjectIdentity`] port);
//! 2. `company.yaml` (an empty folder names the first-run wizard; nothing
//!    is created, the runtime base included);
//! 3. the runtime base's lock, held only while resolving (identity
//!    re-checked under it);
//! 4. root and id: normal, a copy, a move, an unmounted recorded volume, a
//!    changed or removed id, a second Mac or a restored project, or new;
//! 5. the legacy in-project folder (refused while its lock is held, or when
//!    it sits beside a runtime `state.db` with no unfinished move journal);
//! 6. the database version (read-only);
//! 7. the marker (another Mac's id refuses naming it; this Mac's id with a
//!    live pid refuses naming the holder; a gone pid is replaced);
//! 8. the decision flags (`--confirm-move`, `--take-over`): refused naming
//!    the UI while the config enables a stdio MCP server or a Source with
//!    `render: true`; otherwise asked on the terminal, No the default;
//! 9. the writes: the mint, the runtime folder (0700, files 0600), the root
//!    record, then the index; or the legacy move under its journal; the
//!    store (migrations, backups); `ConfigIdentityAssign` when minted; the
//!    audits; the marker; then the base lock is released.

use std::fs::File;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use am_config::{ConfigIdentityAssign, ConfigWriter};
use am_domain::runtime::{
    APP_DATA_NAME, AppDataLayout, RUNTIME_BASE_NAME, legacy_dir, open_marker,
};
use am_domain::{
    Actor, IdentityError, MacId, ProjectId, ProjectIdentity, RootIdentity, RuntimeBase,
    RuntimeLayout, VolumeState,
};
use am_store::legacy::{self, LegacyMove, Phase};
use am_store::{OpenReport, Store, StoreConfigHistory, WriteCommand};

/// Why a project was not opened, in words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn refuse<T>(words: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal(words.into()))
}

fn words(e: impl std::fmt::Display) -> Refusal {
    Refusal(e.to_string())
}

/// The Human actor of the CLI's decisions.
pub fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

/// The System actor of the runtime resolver.
pub fn runtime_actor() -> Actor {
    Actor::System {
        component: "runtime".into(),
    }
}

// ---------------------------------------------------------------- places

/// Where runtime folders, the app data dir and the cloud roots are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    pub base: RuntimeBase,
    /// `<OS app data>/agentic-manager/`: this Mac's id, the run file.
    pub app_data: PathBuf,
    /// Every path in the app data dir (AD-17): the user Accounts file, the
    /// user store and its lock, built from `app_data`.
    pub user: AppDataLayout,
    /// AD-30's cloud roots: a project under one leaves its legacy folder by
    /// a checked copy, never a rename.
    pub cloud_roots: Vec<PathBuf>,
}

impl Places {
    /// The places inside the OS app-data folder `app_support` (macOS:
    /// `~/Library/Application Support`; elsewhere `$XDG_DATA_HOME`), with
    /// the cloud roots under `home`.
    pub fn under(app_support: &Path, home: &Path) -> Places {
        let app_data = app_support.join(APP_DATA_NAME);
        Places {
            base: RuntimeBase::new(app_support.join(RUNTIME_BASE_NAME)),
            user: AppDataLayout::new(app_data.clone()),
            app_data,
            cloud_roots: vec![
                home.join("Library").join("CloudStorage"),
                home.join("Library")
                    .join("Mobile Documents")
                    .join("com~apple~CloudDocs"),
            ],
        }
    }

    /// This platform's places (macOS: under `~/Library/Application
    /// Support/`; elsewhere under `$XDG_DATA_HOME`, by default
    /// `~/.local/share`).
    pub fn from_env() -> Result<Places, Refusal> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|h| h.is_absolute())
            .ok_or_else(|| {
                Refusal(
                    "HOME is not set to a folder, so the runtime folders cannot be found".into(),
                )
            })?;
        #[cfg(target_os = "macos")]
        let app_support = home.join("Library").join("Application Support");
        #[cfg(not(target_os = "macos"))]
        let app_support = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".local").join("share"));
        Ok(Places::under(&app_support, &home))
    }

    fn under_cloud_root(&self, root: &Path) -> bool {
        self.cloud_roots.iter().any(|c| {
            let c = c.canonicalize().unwrap_or_else(|_| c.clone());
            root.starts_with(&c)
        })
    }
}

// ---------------------------------------------------------------- files

#[cfg(unix)]
fn private_dir_builder() -> std::fs::DirBuilder {
    use std::os::unix::fs::DirBuilderExt;
    let mut b = std::fs::DirBuilder::new();
    b.mode(0o700);
    b
}

#[cfg(not(unix))]
fn private_dir_builder() -> std::fs::DirBuilder {
    std::fs::DirBuilder::new()
}

/// Creates `dir` 0700 when missing (its parents as the OS makes them).
pub(crate) fn ensure_private_dir(dir: &Path) -> Result<(), Refusal> {
    if dir.is_dir() {
        return Ok(());
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Refusal(format!("cannot create {}: {e}", parent.display())))?;
    }
    match private_dir_builder().create(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return refuse(format!("cannot create {}: {e}", dir.display())),
    }
    Ok(())
}

pub(crate) fn private_options() -> std::fs::OpenOptions {
    let mut o = std::fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}

/// Writes `bytes` to `path` by an owner-only temp file and a rename.
pub(crate) fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), Refusal> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut f = private_options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        if let Some(dir) = path.parent() {
            File::open(dir)?.sync_all()?;
        }
        Ok(())
    })();
    result.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Refusal(format!("cannot write {}: {e}", path.display()))
    })
}

/// Records the port `agentic serve` bound in the runtime folder,
/// owner-only (story 2.2, AD-12): every egress client built for the
/// project reads it and refuses it to Accounts and MCP servers.
pub fn record_serve_port(layout: &RuntimeLayout, port: u16) -> Result<(), Refusal> {
    replace_file(&layout.serve_port(), format!("{port}\n").as_bytes())
}

/// Removes the recorded serving port (at `serve`'s exit).
pub fn clear_serve_port(layout: &RuntimeLayout) {
    let _ = std::fs::remove_file(layout.serve_port());
}

/// Records, owner-only, the port this `agentic serve` process listens on in
/// the user-level folder of running servers
/// ([`AppDataLayout::serving_dir`]), named by its process id, with or
/// without a project: every egress directory refuses every recorded port
/// to Accounts and MCP servers (story 2.2, AD-12).
pub fn record_serving(app_data: &Path, port: u16) -> Result<(), Refusal> {
    let dir = AppDataLayout::new(app_data.to_path_buf()).serving_dir();
    // The app data dir is created owner-only, then the folder in it.
    ensure_private_dir(app_data)?;
    ensure_private_dir(&dir)?;
    replace_file(
        &dir.join(std::process::id().to_string()),
        format!("{port}\n").as_bytes(),
    )
}

/// Removes this process's record of its port (at `serve`'s exit).
pub fn clear_serving(app_data: &Path) {
    let _ = std::fs::remove_file(
        AppDataLayout::new(app_data.to_path_buf())
            .serving_dir()
            .join(std::process::id().to_string()),
    );
}

/// The ports the running `agentic serve` processes recorded. A record whose
/// process is not running is ignored and removed, and so is a CLI token
/// file (`<pid>.token`, story 3.2) whose process is not running; anything
/// that is not a process id holding a port is ignored. `agentic serve`
/// calls it at start to prune what a crashed server left.
pub fn serving_ports(app_data: &Path) -> Vec<u16> {
    let Ok(entries) = std::fs::read_dir(AppDataLayout::new(app_data.to_path_buf()).serving_dir())
    else {
        return Vec::new();
    };
    let mut ports = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if let Some(pid) = name
            .strip_suffix(".token")
            .and_then(|p| p.parse::<u32>().ok())
        {
            if !pid_alive(pid) {
                let _ = std::fs::remove_file(entry.path());
            }
            continue;
        }
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if !pid_alive(pid) {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        if let Some(port) = std::fs::read_to_string(entry.path())
            .ok()
            .and_then(|t| t.trim().parse::<u16>().ok())
        {
            ports.push(port);
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

// ---------------------------------------------------------------- mac id

/// This Mac's id from `<app data>/mac-id`: `None` when it was never minted.
/// A file that cannot be read or does not hold a UUID is refused in words
/// and never re-minted.
pub fn read_mac_id(app_data: &Path) -> Result<Option<MacId>, Refusal> {
    let path = AppDataLayout::new(app_data.to_path_buf()).mac_id();
    match std::fs::read_to_string(&path) {
        Ok(text) => MacId::parse(text.trim()).map(Some).ok_or_else(|| {
            Refusal(format!(
                "this Mac's id file {} does not hold an id; it is never made again on its own, because projects open here are marked with it: restore it from a backup, or remove it only if no project is open on this Mac",
                path.display()
            ))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => refuse(format!(
            "this Mac's id file {} cannot be read: {e}",
            path.display()
        )),
    }
}

/// Mints this Mac's random id once: `<app data>/mac-id`, created
/// exclusively with mode 0600. A file made meanwhile is read instead.
pub fn mint_mac_id(app_data: &Path) -> Result<MacId, Refusal> {
    ensure_private_dir(app_data)?;
    let path = AppDataLayout::new(app_data.to_path_buf()).mac_id();
    let id = MacId::mint();
    match private_options().write(true).create_new(true).open(&path) {
        Ok(mut f) => {
            f.write_all(format!("{id}\n").as_bytes())
                .and_then(|()| f.sync_all())
                .map_err(|e| Refusal(format!("cannot write {}: {e}", path.display())))?;
            Ok(id)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            read_mac_id(app_data)?.ok_or_else(|| Refusal(format!("{} vanished", path.display())))
        }
        Err(e) => refuse(format!("cannot create {}: {e}", path.display())),
    }
}

// ---------------------------------------------------------------- roots

/// A runtime folder's record of its project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootRecord {
    pub project_id: ProjectId,
    pub root: RootIdentity,
}

impl RootRecord {
    fn to_text(&self) -> String {
        format!(
            "project_id: {}\nvolume: {}\nfile_id: {}\npath: {}\n",
            self.project_id,
            self.root.volume,
            self.root.file_id,
            hex_of(self.root.path.as_os_str().as_encoded_bytes())
        )
    }

    fn parse(text: &str) -> Option<RootRecord> {
        let field = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix(": "))
        };
        let path = unhex(field("path")?)?;
        Some(RootRecord {
            project_id: ProjectId::parse(field("project_id")?)?,
            root: RootIdentity {
                volume: field("volume")?.to_string(),
                file_id: field("file_id")?.parse().ok()?,
                path: PathBuf::from(String::from_utf8(path).ok()?),
            },
        })
    }
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The runtime folder's root record, if it has a readable one.
pub fn read_record(layout: &RuntimeLayout) -> Option<RootRecord> {
    std::fs::read_to_string(layout.root_record())
        .ok()
        .and_then(|t| RootRecord::parse(&t))
}

/// Every runtime folder's root record (a record naming another id than its
/// folder is ignored).
pub fn records(base: &RuntimeBase) -> Vec<RootRecord> {
    let Ok(read) = std::fs::read_dir(base.dir()) else {
        return Vec::new();
    };
    let mut out: Vec<RootRecord> = read
        .flatten()
        .filter_map(|e| {
            let id = RuntimeBase::project_of(&e.file_name().to_string_lossy())?;
            let rec = read_record(&base.project(&id))?;
            (rec.project_id == id).then_some(rec)
        })
        .collect();
    out.sort_by_key(|r| r.project_id);
    out
}

/// The root index's text for `records`.
fn index_text(records: &[RootRecord]) -> String {
    records
        .iter()
        .map(|r| format!("{}\t{}\t{}\n", r.root.volume, r.root.file_id, r.project_id))
        .collect()
}

/// The index's lines, as (volume, file id, project id).
fn index_lines(base: &RuntimeBase) -> Vec<(String, u64, ProjectId)> {
    std::fs::read_to_string(base.root_index())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let volume = f.next()?.to_string();
            let file_id = f.next()?.parse().ok()?;
            let id = ProjectId::parse(f.next()?)?;
            Some((volume, file_id, id))
        })
        .collect()
}

/// The project the index lists for `root` when that project's runtime
/// folder exists but its root record cannot be read (so `records` skips it).
fn unreadable_indexed(base: &RuntimeBase, root: &RootIdentity) -> Option<ProjectId> {
    index_lines(base)
        .into_iter()
        .find(|(v, f, id)| {
            *v == root.volume
                && *f == root.file_id
                && base.project(id).dir().is_dir()
                && read_record(&base.project(id)).is_none_or(|r| r.project_id != *id)
        })
        .map(|(_, _, id)| id)
}

/// Rewrites the base's root index from the records when it is missing or
/// stale. A line whose runtime folder still exists but whose record cannot
/// be read is kept, so that root is never taken for a new one.
fn refresh_index(base: &RuntimeBase) -> Result<(), Refusal> {
    let mut want = index_text(&records(base));
    for (volume, file_id, id) in index_lines(base) {
        let layout = base.project(&id);
        if layout.dir().is_dir() && read_record(&layout).is_none_or(|r| r.project_id != id) {
            want.push_str(&format!("{volume}\t{file_id}\t{id}\n"));
        }
    }
    if std::fs::read_to_string(base.root_index()).ok().as_deref() != Some(want.as_str()) {
        replace_file(&base.root_index(), want.as_bytes())?;
    }
    Ok(())
}

/// Records `root` in its runtime folder, then the index (each by temp file
/// and rename; the record first).
fn record_root(base: &RuntimeBase, rec: &RootRecord) -> Result<(), Refusal> {
    let layout = base.project(&rec.project_id);
    replace_file(&layout.root_record(), rec.to_text().as_bytes())?;
    refresh_index(base)
}

// ---------------------------------------------------------------- marker

/// The open marker's contents: no state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub mac_name: String,
    pub mac_id: String,
    pub pid: u32,
    pub opened_at: String,
}

impl Marker {
    fn to_text(&self) -> String {
        format!(
            "mac_name: {}\nmac_id: {}\npid: {}\nopened_at: {}\n",
            self.mac_name.replace(['\n', '\r'], " "),
            self.mac_id,
            self.pid,
            self.opened_at
        )
    }

    pub fn parse(text: &str) -> Option<Marker> {
        let field = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix(": "))
                .map(str::to_string)
        };
        Some(Marker {
            mac_name: field("mac_name")?,
            mac_id: field("mac_id")?,
            pid: field("pid")?.parse().ok()?,
            opened_at: field("opened_at")?,
        })
    }
}

/// Removes this process's marker when dropped (a clean close, a signal or
/// an error after it was written), only when its id, pid and opened-at are
/// still this process's own.
#[derive(Debug)]
pub struct MarkerGuard {
    path: PathBuf,
    own: Marker,
}

impl MarkerGuard {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for MarkerGuard {
    fn drop(&mut self) {
        let mine = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| Marker::parse(&t))
            .is_some_and(|m| {
                m.mac_id == self.own.mac_id
                    && m.pid == self.own.pid
                    && m.opened_at == self.own.opened_at
            });
        if mine {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// True while process `pid` runs on this Mac.
fn pid_alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        Path::new("/proc").join(pid.to_string()).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::process::Command::new("/bin/ps")
            .args(["-p", &pid.to_string(), "-o", "pid="])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .is_ok_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty())
    }
}

// ---------------------------------------------------------------- prompt

/// How the CLI asks a decision (`--confirm-move`, `--take-over`).
pub trait Prompt {
    /// True only when stdin and stdout are terminals.
    fn interactive(&self) -> bool;
    /// Asks `question`; No is the default.
    fn ask(&mut self, question: &str) -> bool;
}

/// The terminal: asks on stdout, reads one line from stdin.
pub struct TerminalPrompt;

impl Prompt for TerminalPrompt {
    fn interactive(&self) -> bool {
        crate::terminal::interactive()
    }

    fn ask(&mut self, question: &str) -> bool {
        print!("{question} [y/N] ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line).is_err() {
            return false;
        }
        matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    }
}

// ---------------------------------------------------------------- open

/// What the owner asked for on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    pub root: PathBuf,
    /// `--confirm-move`: re-point a root recorded on an unmounted volume.
    pub confirm_move: bool,
    /// `--take-over`: open although another Mac's marker is present.
    pub take_over: bool,
}

/// An open project. Fields drop in order: the marker first, then the config
/// writer and the store (whose writer thread holds the project lock).
pub struct Opened {
    pub marker: MarkerGuard,
    pub writer: ConfigWriter,
    pub store: Store,
    pub layout: RuntimeLayout,
    pub project_id: ProjectId,
    /// The report, as printed (migrations, the legacy move, reconciled writes).
    pub lines: Vec<String>,
}

/// The held runtime-base lock.
struct BaseLock(File);

impl Drop for BaseLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn take_base_lock(base: &RuntimeBase) -> Result<BaseLock, Refusal> {
    ensure_private_dir(base.dir())?;
    let path = base.lock();
    let f = private_options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| Refusal(format!("cannot open {}: {e}", path.display())))?;
    f.lock()
        .map_err(|e| Refusal(format!("cannot lock {}: {e}", path.display())))?;
    Ok(BaseLock(f))
}

/// How the root and its id were classified (check 4).
enum Class {
    Normal(ProjectId),
    /// No id and not indexed: mint one (in memory until the writes).
    New(ProjectId),
    /// The id's runtime folder recorded a root that is gone (System) or on
    /// an unmounted volume the owner confirmed (Human).
    Move {
        id: ProjectId,
        from: RootIdentity,
        human: bool,
    },
    /// Id present, not indexed, no runtime folder records it.
    SecondMac(ProjectId),
    /// Indexed, no id written yet, and an unfinished legacy journal.
    ResumeLegacy(ProjectId),
}

impl Class {
    fn id(&self) -> ProjectId {
        match self {
            Class::Normal(id)
            | Class::New(id)
            | Class::SecondMac(id)
            | Class::ResumeLegacy(id)
            | Class::Move { id, .. } => *id,
        }
    }
}

/// A decision asked at check 8.
enum Decision {
    ConfirmMove { from: RootIdentity },
    TakeOver { other: Marker },
}

fn identity_words(e: IdentityError) -> Refusal {
    Refusal(e.to_string())
}

fn store_words(e: am_store::StoreError) -> Refusal {
    Refusal(e.to_string())
}

/// Opens the project at `req.root` (AD-32's order; see the module docs).
pub fn open_project(
    places: &Places,
    identity: &dyn ProjectIdentity,
    req: &OpenRequest,
    prompt: &mut dyn Prompt,
) -> Result<Opened, Refusal> {
    let root = match req.root.canonicalize() {
        Ok(r) if r.is_dir() => r,
        Ok(r) => {
            return refuse(format!(
                "{} is not a folder; give the project folder",
                r.display()
            ));
        }
        Err(_) => {
            return refuse(format!(
                "the project folder {} does not exist",
                req.root.display()
            ));
        }
    };
    // 1. The identity tools.
    let id_root = identity.root(&root).map_err(identity_words)?;
    let mac_name = identity.mac_name().map_err(identity_words)?;
    // 2. company.yaml (nothing is created before this passes).
    let head = am_config::read_company(&root).map_err(words)?;
    // 3. The base lock; the identity re-checked under it.
    let _base_lock = take_base_lock(&places.base)?;
    let again = identity.root(&root).map_err(identity_words)?;
    if !again.same_root(&id_root) {
        return refuse(format!(
            "the project folder {} changed while it was being opened; open it again",
            root.display()
        ));
    }
    // 4. Root and id.
    let all = records(&places.base);
    let indexed = all.iter().find(|r| r.root.same_root(&id_root));
    if indexed.is_none()
        && let Some(lost) = unreadable_indexed(&places.base, &id_root)
    {
        return refuse(format!(
            "this folder is in the root index as project {lost}, but that project's root record {} cannot be read; nothing is opened and no new id is made. Restore the record (or move the runtime folder {} aside if it is not this project's), then open the project again",
            places.base.project(&lost).root_record().display(),
            places.base.project(&lost).dir().display()
        ));
    }
    let mut decisions = Vec::new();
    let class = match (indexed, head.project_id) {
        (Some(rec), Some(id)) if rec.project_id == id => Class::Normal(id),
        (Some(rec), found) => {
            let layout = places.base.project(&rec.project_id);
            let journal = legacy::read_journal(&layout).map_err(store_words)?;
            if found.is_none() && journal.is_some_and(|j| j.unfinished()) {
                Class::ResumeLegacy(rec.project_id)
            } else {
                let found = found
                    .map(|f| format!("'{f}'"))
                    .unwrap_or_else(|| "none (the line was removed)".into());
                return refuse(format!(
                    "this folder's project id changed: its runtime folder {} belongs to project id '{}', but company.yaml now has {found}; nothing is opened and no new id is made. Either write the id back (set `project_id: {}` in company.yaml, right after schema_version) to keep this project's state, or treat the folder as a new project by removing the project_id line, which starts it with an empty runtime folder",
                    layout.dir().display(),
                    rec.project_id,
                    rec.project_id
                ));
            }
        }
        (None, Some(id)) => match all.iter().find(|r| r.project_id == id) {
            None => Class::SecondMac(id),
            Some(other) => match identity.locate(&other.root).map_err(identity_words)? {
                VolumeState::Unmounted => {
                    if !req.confirm_move {
                        return refuse(format!(
                            "project {id} was last opened at {}, on a volume that is not mounted now, so this folder may be a copy or the project moved; mount that volume and open the project from there, or, if this folder is the project now, open it with `agentic serve --project {} --confirm-move` in a terminal",
                            other.root.path.display(),
                            root.display()
                        ));
                    }
                    decisions.push(Decision::ConfirmMove {
                        from: other.root.clone(),
                    });
                    Class::Move {
                        id,
                        from: other.root.clone(),
                        human: true,
                    }
                }
                VolumeState::Mounted { root: Some(there) } => {
                    // The original is still there: whether it still holds
                    // the id decides copy or move, so a read that fails is
                    // refused, never taken as "does not hold it".
                    let holds = match am_config::read_company(&there) {
                        Ok(h) => h.project_id == Some(id),
                        Err(e) => {
                            return refuse(format!(
                                "project {id} was last opened at {}, which is still there, but its company.yaml cannot be read ({e}), so this app cannot tell whether {} is a copy of it or the project moved; nothing is opened. Make that company.yaml readable and open the project again",
                                other.root.path.display(),
                                root.display()
                            ));
                        }
                    };
                    if holds {
                        return refuse(format!(
                            "this folder ({}) is a copy of the project at {}: both hold project id {id}, and its runtime state belongs to the original. To treat this copy as a new project, remove the `project_id` line from this folder's company.yaml and open it again; it then gets its own id and an empty runtime folder",
                            root.display(),
                            other.root.path.display()
                        ));
                    }
                    Class::Move {
                        id,
                        from: other.root.clone(),
                        human: false,
                    }
                }
                VolumeState::Mounted { root: None } => Class::Move {
                    id,
                    from: other.root.clone(),
                    human: false,
                },
            },
        },
        (None, None) => Class::New(ProjectId::mint()),
    };
    let id = class.id();
    let layout = places.base.project(&id);
    // 5. The legacy folder.
    let old = legacy_dir(&root);
    let journal = legacy::read_journal(&layout).map_err(store_words)?;
    let unfinished = journal.as_ref().is_some_and(|j| j.unfinished());
    let has_legacy = old.is_dir();
    if has_legacy {
        legacy::check_lock(&old).map_err(store_words)?;
        if layout.state_db().exists() && !unfinished {
            return refuse(format!(
                "the project has both an old runtime folder in the project ({}) and runtime state at {}, which this app did not move there; nothing is opened. Keep the one you want: move the other out of the way, then open the project again",
                old.display(),
                layout.dir().display()
            ));
        }
    }
    let legacy_move = (has_legacy || unfinished).then(|| {
        let (pid, minted) = match &journal {
            Some(j) if unfinished => (j.project_id.clone(), j.id_minted),
            _ => (id.to_string(), matches!(class, Class::New(_))),
        };
        LegacyMove {
            project_root: root.clone(),
            layout: layout.clone(),
            project_id: pid,
            id_minted: minted,
            copy_always: places.under_cloud_root(&root),
        }
    });
    // 6. The database version.
    am_store::check_database(&layout.state_db()).map_err(store_words)?;
    if has_legacy {
        am_store::check_database(&RuntimeLayout::at(old.clone()).state_db())
            .map_err(store_words)?;
    }
    // 7. The marker.
    let mac_id = read_mac_id(&places.app_data)?;
    let marker_path = open_marker(&root);
    if marker_path.exists() {
        let found = std::fs::read_to_string(&marker_path)
            .ok()
            .and_then(|t| Marker::parse(&t));
        let ours = found
            .as_ref()
            .is_some_and(|m| mac_id.is_some_and(|me| m.mac_id == me.to_string()));
        match found {
            Some(m) if ours => {
                // Live only while its process still holds the store lock: a
                // pid reused after a crash or a reboot holds nothing, and its
                // marker is left over and replaced.
                if pid_alive(m.pid) && am_store::lock_is_held(&layout) {
                    let holder = am_store::lock_holder(&layout)
                        .filter(|h| h.pid == m.pid)
                        .map(|h| h.to_string())
                        .unwrap_or_else(|| {
                            format!("pid {} on this Mac, opened {}", m.pid, m.opened_at)
                        });
                    return refuse(format!(
                        "the project is already open on this Mac ({holder}); close it there first"
                    ));
                }
            }
            other => {
                let other = other.unwrap_or(Marker {
                    mac_name: "an unknown Mac (the marker cannot be read)".into(),
                    mac_id: String::new(),
                    pid: 0,
                    opened_at: "at an unknown time".into(),
                });
                if !req.take_over {
                    return refuse(format!(
                        "the project is open on another Mac, {} (since {}); close it there first. If that Mac is off or the marker is left over, open it here with `agentic serve --project {} --take-over` in a terminal",
                        other.mac_name,
                        other.opened_at,
                        root.display()
                    ));
                }
                decisions.push(Decision::TakeOver { other });
            }
        }
    }
    // 8. The decision flags.
    if !decisions.is_empty() {
        if let Some(why) = am_config::decisions_locked(&root) {
            return refuse(format!(
                "--confirm-move and --take-over cannot be used from the command line while {why}; open the project in the app (the UI) and decide there"
            ));
        }
        if !prompt.interactive() {
            return refuse(
                "--confirm-move and --take-over need a terminal: run `agentic serve` in a terminal to answer the question",
            );
        }
        for d in &decisions {
            let (question, no) = match d {
                Decision::ConfirmMove { from } => (
                    format!(
                        "Project {id} was last opened at {} on a volume that is not mounted. Make {} the project now (its runtime state moves with it)?",
                        from.path.display(),
                        root.display()
                    ),
                    "the move was not confirmed; the project stays closed",
                ),
                Decision::TakeOver { other } => (
                    format!(
                        "The project is marked open on {} (since {}). Open it here anyway?",
                        other.mac_name, other.opened_at
                    ),
                    "not taken over; the project stays closed",
                ),
            };
            if !prompt.ask(&question) {
                return refuse(no);
            }
        }
    }
    // 9. The writes.
    let mac_id = match mac_id {
        Some(m) => m,
        None => mint_mac_id(&places.app_data)?,
    };
    let this_root = RootRecord {
        project_id: id,
        root: RootIdentity {
            path: root.clone(),
            ..id_root.clone()
        },
    };
    match &class {
        Class::New(_) | Class::SecondMac(_) | Class::Move { .. } => {
            ensure_private_dir(layout.dir())?;
            record_root(&places.base, &this_root)?;
        }
        Class::Normal(_) | Class::ResumeLegacy(_) => {
            // A folder renamed or moved on its volume keeps its file id; its
            // record follows it, so later refusals name where it is now.
            if indexed.is_some_and(|r| r.root.path != root) {
                record_root(&places.base, &this_root)?;
            } else {
                refresh_index(&places.base)?;
            }
        }
    }
    #[cfg(feature = "test-support")]
    if testing::failing(testing::FailPoint::AfterRootRecord) {
        return refuse("simulated crash after the root record");
    }
    let kept = match &legacy_move {
        Some(m) => Some(legacy::move_legacy(m).map_err(store_words)?),
        None => None,
    };
    #[cfg(feature = "test-support")]
    if kept.is_some() && testing::failing(testing::FailPoint::AfterLegacyMove) {
        return refuse("simulated failure after the legacy move");
    }
    let (store, mut report): (Store, OpenReport) =
        am_store::open_runtime(&layout, mac_name.clone()).map_err(store_words)?;
    report.legacy_kept = kept.as_ref().map(|k| k.kept.clone());
    let mut lines = report.lines();
    if lines.is_empty() {
        lines.push(format!("database at version {}", report.version));
    }
    let history = StoreConfigHistory::new(&store).map_err(store_words)?;
    let writer = ConfigWriter::open(&root, std::sync::Arc::new(history)).map_err(words)?;
    let reconciled = writer
        .reconcile(Actor::System {
            component: "config-writer".into(),
        })
        .map_err(words)?;
    for entry in &reconciled.completed {
        lines.push(format!(
            "completed the interrupted write of {} (History entry {})",
            entry.file, entry.id
        ));
    }
    for dropped in &reconciled.dropped {
        lines.push(format!("dropped the unwritten config change {dropped}"));
    }
    let minted = match &legacy_move {
        Some(m) => m.id_minted,
        None => matches!(class, Class::New(_)),
    };
    let head_now = am_config::read_company(&root).map_err(words)?;
    if minted && head_now.project_id.is_none() {
        writer
            .assign_identity(
                runtime_actor(),
                ConfigIdentityAssign {
                    project_id: id.to_string(),
                },
            )
            .map_err(words)?;
        lines.push(format!("gave the project its id {id} (company.yaml)"));
    }
    if legacy_move.is_some() && minted {
        legacy::mark(&layout, Phase::IdWritten).map_err(store_words)?;
    }
    let audit = |actor: Actor, action: &str, outcome: String| {
        store
            .writer()
            .exec(
                actor,
                WriteCommand::Audit {
                    action: action.into(),
                    target: id.to_string(),
                    outcome,
                    hash: None,
                    args: Default::default(),
                },
            )
            .map_err(store_words)
    };
    if let Class::Move { from, human, .. } = &class {
        let actor = if *human { owner() } else { runtime_actor() };
        audit(
            actor,
            "project.moved",
            format!("from {} to {}", from.path.display(), root.display()),
        )?;
        lines.push(format!(
            "the project moved from {} to {}",
            from.path.display(),
            root.display()
        ));
    }
    for d in &decisions {
        if let Decision::TakeOver { other } = d {
            audit(
                owner(),
                "project.taken_over",
                format!("from {} (opened {})", other.mac_name, other.opened_at),
            )?;
            lines.push(format!("opened here, taken over from {}", other.mac_name));
        }
    }
    if let Some(k) = &kept {
        audit(
            runtime_actor(),
            "project.legacy_moved",
            format!("kept at {}", k.kept.display()),
        )?;
        legacy::mark(&layout, Phase::Done).map_err(store_words)?;
    }
    let own = Marker {
        mac_name,
        mac_id: mac_id.to_string(),
        pid: std::process::id(),
        opened_at: am_store::ts(jiff::Timestamp::now()),
    };
    std::fs::write(&marker_path, own.to_text())
        .map_err(|e| Refusal(format!("cannot write {}: {e}", marker_path.display())))?;
    Ok(Opened {
        marker: MarkerGuard {
            path: marker_path,
            own,
        },
        writer,
        store,
        layout,
        project_id: id,
        lines,
    })
}

// ---------------------------------------------------------------- read-only

/// The runtime folder of the project at `root` on this Mac, found without a
/// lock, a write or an identity tool (`agentic audit`): refused in words
/// when there is no `company.yaml`, no `project_id`, an unmoved legacy
/// folder, or no runtime folder on this Mac.
pub fn find_runtime(places: &Places, root: &Path) -> Result<RuntimeLayout, Refusal> {
    let head = am_config::read_company(root).map_err(words)?;
    let Some(id) = head.project_id else {
        return refuse(format!(
            "{} has no project_id in company.yaml yet; open the project once with `agentic serve --project {}`",
            root.display(),
            root.display()
        ));
    };
    let old = legacy_dir(root);
    if old.is_dir() {
        return refuse(format!(
            "{} still holds the old runtime folder {}; open the project once with `agentic serve --project {}` to move it",
            root.display(),
            old.display(),
            root.display()
        ));
    }
    let layout = places.base.project(&id);
    if !layout.dir().is_dir() {
        return refuse(format!(
            "project {id} has no runtime folder on this Mac ({}); open it here once with `agentic serve --project {}`",
            layout.dir().display(),
            root.display()
        ));
    }
    Ok(layout)
}

/// A project opened for the interim CLI Account edit (story 2.1, AD-15):
/// the store under its lock and the config writer, with nothing about the
/// project or its runtime folder changed to get there. Fields drop in
/// order: the writer, then the store (whose writer thread holds the lock).
pub struct OpenedUnchanged {
    pub writer: ConfigWriter,
    pub store: Store,
    pub layout: RuntimeLayout,
    pub project_id: ProjectId,
    /// The project root, canonicalised.
    pub root: PathBuf,
    /// What settling interrupted config writes did, as printed.
    pub lines: Vec<String>,
}

/// Opens the project at `root` as the one writer for the interim CLI
/// Account edit (story 2.1, AD-15), by AD-32's checks in their order and
/// read-only: it mints no `project_id`, creates no runtime folder, records
/// no root, moves no legacy folder, creates, backs up or migrates no
/// database and writes no marker; whatever would need one of those is
/// refused in words naming `agentic serve`, which does them. The store's
/// lock is then held for as long as the result lives (a holder is named in
/// the refusal). The only writes are the lock files and, like every open of
/// the writer, the settling of a config write a crash interrupted (History
/// and audit in the store; stale temp files removed).
pub fn open_project_unchanged(
    places: &Places,
    identity: &dyn ProjectIdentity,
    root: &Path,
) -> Result<OpenedUnchanged, Refusal> {
    let root = match root.canonicalize() {
        Ok(r) if r.is_dir() => r,
        Ok(r) => {
            return refuse(format!(
                "{} is not a folder; give the project folder",
                r.display()
            ));
        }
        Err(_) => {
            return refuse(format!(
                "the project folder {} does not exist",
                root.display()
            ));
        }
    };
    let serve = format!("`agentic serve --project {}`", root.display());
    // 1. The identity tools.
    let id_root = identity.root(&root).map_err(identity_words)?;
    let mac_name = identity.mac_name().map_err(identity_words)?;
    // 2. company.yaml and its id (never minted here).
    let head = am_config::read_company(&root).map_err(words)?;
    let Some(id) = head.project_id else {
        return refuse(format!(
            "{} has no project_id in company.yaml yet, and this command never gives a project its id; open the project once with {serve}, then run this command again",
            root.display()
        ));
    };
    // 3. The base lock, only where a runtime base already is.
    let layout = places.base.project(&id);
    let not_here = || {
        Refusal(format!(
            "project {id} has no runtime folder on this Mac ({}), and this command never creates one; open the project here once with {serve}, then run this command again",
            layout.dir().display()
        ))
    };
    if !places.base.dir().is_dir() {
        return Err(not_here());
    }
    let _base_lock = take_base_lock(&places.base)?;
    let again = identity.root(&root).map_err(identity_words)?;
    if !again.same_root(&id_root) {
        return refuse(format!(
            "the project folder {} changed while it was being opened; run the command again",
            root.display()
        ));
    }
    // 4. Root and id: only a root this Mac records under this id.
    let all = records(&places.base);
    match all.iter().find(|r| r.root.same_root(&id_root)) {
        Some(rec) if rec.project_id == id => {}
        Some(rec) => {
            return refuse(format!(
                "this folder's project id changed: its runtime folder {} belongs to project id '{}', but company.yaml now has '{id}'; nothing is opened. Settle it by opening the project with {serve}",
                places.base.project(&rec.project_id).dir().display(),
                rec.project_id
            ));
        }
        None if layout.dir().is_dir() => {
            return refuse(format!(
                "this folder is not the one this Mac records for project {id} (it moved, or it is a copy), and this command never records a root; open the project once with {serve} to settle it, then run this command again"
            ));
        }
        None => return Err(not_here()),
    }
    // 5. The legacy folder (never moved here).
    let old = legacy_dir(&root);
    let unfinished = legacy::read_journal(&layout)
        .map_err(store_words)?
        .is_some_and(|j| j.unfinished());
    if old.is_dir() || unfinished {
        return refuse(format!(
            "{} still holds the old runtime folder {} (or its move out of the project is unfinished), and this command never moves it; open the project once with {serve} to move it, then run this command again",
            root.display(),
            old.display()
        ));
    }
    // 6. The database version (a newer one; an older one is refused below,
    // under the lock, without being migrated).
    am_store::check_database(&layout.state_db()).map_err(store_words)?;
    // 7. The marker: another Mac's refuses; this command writes none.
    let marker_path = open_marker(&root);
    if marker_path.exists() {
        let mac_id = read_mac_id(&places.app_data)?;
        let found = std::fs::read_to_string(&marker_path)
            .ok()
            .and_then(|t| Marker::parse(&t));
        let ours = found
            .as_ref()
            .is_some_and(|m| mac_id.is_some_and(|me| m.mac_id == me.to_string()));
        if !ours {
            let (name, since) = match &found {
                Some(m) => (m.mac_name.as_str(), m.opened_at.as_str()),
                None => (
                    "an unknown Mac (the marker cannot be read)",
                    "an unknown time",
                ),
            };
            return refuse(format!(
                "the project is open on another Mac, {name} (since {since}); close it there first. This command never takes a project over: if that Mac is off or the marker is left over, settle it with `agentic serve --project {} --take-over` in a terminal",
                root.display()
            ));
        }
        // This Mac's marker: a live holder is refused by the store's lock,
        // naming it; a left-over one is the next `serve`'s to replace.
    }
    let store = am_store::open_runtime_unchanged(&layout, mac_name).map_err(store_words)?;
    let history = StoreConfigHistory::new(&store).map_err(store_words)?;
    let writer = ConfigWriter::open(&root, std::sync::Arc::new(history)).map_err(words)?;
    let reconciled = writer
        .reconcile(Actor::System {
            component: "config-writer".into(),
        })
        .map_err(words)?;
    let mut lines = Vec::new();
    for entry in &reconciled.completed {
        lines.push(format!(
            "completed the interrupted write of {} (History entry {})",
            entry.file, entry.id
        ));
    }
    for dropped in &reconciled.dropped {
        lines.push(format!("dropped the unwritten config change {dropped}"));
    }
    // Tests only: stop this writer's writes as a crash would.
    #[cfg(feature = "test-support")]
    writer.fail_at(testing::writer_fail_point(&root));
    Ok(OpenedUnchanged {
        writer,
        store,
        layout,
        project_id: id,
        root,
        lines,
    })
}

/// The runtime folder's `knowledge/` for `agentic validate` on a project,
/// when it has one on this Mac (no lock, no write, no identity tool).
pub fn knowledge_root(places: &Places, root: &Path) -> Option<PathBuf> {
    let id = am_config::read_company(root).ok()?.project_id?;
    let layout = places.base.project(&id);
    layout.dir().is_dir().then(|| layout.knowledge_dir())
}

/// Fail points for tests (`test-support` only).
#[cfg(feature = "test-support")]
pub mod testing {
    use std::sync::Mutex;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FailPoint {
        /// After the root record and index, before the store opens and the
        /// id is written (a crash during a plain mint).
        AfterRootRecord,
        /// After the legacy folder moved out, before the store opens and the
        /// move is audited and reported.
        AfterLegacyMove,
    }

    static FAIL: Mutex<Option<FailPoint>> = Mutex::new(None);

    pub fn fail_at(point: Option<FailPoint>) {
        if let Ok(mut f) = FAIL.lock() {
            *f = point;
        }
    }

    pub(crate) fn failing(point: FailPoint) -> bool {
        FAIL.lock().is_ok_and(|f| *f == Some(point))
    }

    /// Where the config writer of the project at each root stops its
    /// writes, for [`super::open_project_unchanged`] (keyed by the root, so
    /// tests in one process do not reach each other's projects).
    static WRITER_FAIL: Mutex<Vec<(std::path::PathBuf, am_config::FailPoint)>> =
        Mutex::new(Vec::new());

    /// Stops the writes of the writer that `open_project_unchanged` builds
    /// for the project at `root` at `point`, as a crash would (story 2.1);
    /// `None` clears it.
    pub fn fail_writer_at(root: &std::path::Path, point: Option<am_config::FailPoint>) {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if let Ok(mut all) = WRITER_FAIL.lock() {
            all.retain(|(r, _)| *r != root);
            if let Some(point) = point {
                all.push((root, point));
            }
        }
    }

    /// What a test runs once in `accounts migrate` of the project at each
    /// root, after the user store's records committed and before the
    /// backup's hash checks (story 2.13).
    type Meanwhile = Box<dyn FnOnce() + Send>;
    static BEFORE_BACKUP_CHECK: Mutex<Vec<(std::path::PathBuf, Meanwhile)>> =
        Mutex::new(Vec::new());

    /// Runs `meanwhile` once in the next `agentic accounts migrate` of the
    /// project at `root`, after the user store's records committed and
    /// before the backup's hash checks (story 2.13), as another process
    /// writing the project's files at that moment would.
    pub fn before_backup_check(root: &std::path::Path, meanwhile: impl FnOnce() + Send + 'static) {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if let Ok(mut all) = BEFORE_BACKUP_CHECK.lock() {
            all.retain(|(r, _)| *r != root);
            all.push((root, Box::new(meanwhile)));
        }
    }

    /// Takes and runs what [`before_backup_check`] set for `root`.
    pub(crate) fn run_before_backup_check(root: &std::path::Path) {
        let found = BEFORE_BACKUP_CHECK.lock().ok().and_then(|mut all| {
            let at = all.iter().position(|(r, _)| r == root)?;
            Some(all.remove(at).1)
        });
        if let Some(meanwhile) = found {
            meanwhile();
        }
    }

    pub(crate) fn writer_fail_point(root: &std::path::Path) -> Option<am_config::FailPoint> {
        WRITER_FAIL
            .lock()
            .ok()?
            .iter()
            .find(|(r, _)| r == root)
            .map(|(_, point)| *point)
    }
}
