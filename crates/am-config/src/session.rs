//! The config session (story 1.9, R2, AD-16): outside edits wait for Reload.
//!
//! A [`ConfigSession`] holds the accepted bytes of every tracked file (see
//! [`crate::tracked`]: the config files and skill reference files the
//! snapshot is read from) and the [`ConfigSnapshot`] built only from them by
//! an exact-map load, so nothing changed on disk applies until the owner
//! reloads. A `notify` watcher on the project folder feeds one worker thread
//! (am-config has no async runtime): each path settles for [`SETTLE`] of
//! quiet, then its sha256 on disk is compared with the accepted bytes, with
//! the last announced hash and with the writer's own writes; a new content is
//! announced once as [`ConfigEvent::ChangedOnDisk`] with its diff, the errors
//! it adds (file, line, fix) and whether it changes `project_id`. A diff whose
//! new bytes hold a secret-looking value is replaced by a worded note. A
//! pending change undone on disk is announced as [`ConfigEvent::Withdrawn`].
//!
//! [`ConfigSession::reload`] applies every pending change as one new
//! snapshot or none: it is refused with nothing pending, for a `project_id`
//! change (AD-16, AD-32) and for a change that adds an error; otherwise each
//! file gets one `outside_edit` History entry through the `ConfigHistory`
//! port (intent, then commit; no file is written). The app's own writes go
//! through [`ConfigSession::apply`], [`ConfigSession::undo`] and
//! [`ConfigSession::restore`], which raise no event, and are refused while a
//! tracked file on disk differs from the accepted bytes.
//!
//! Not here (plan scope): the privilege-widening check of a Reload (epic 4),
//! the banner, the `/api/v1` route and the event stream (epic 5 and the API
//! story).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use am_domain::{Actor, ConfigHistory, HistoryEntry, HistoryError, HistoryKind, NewIntent};
use notify::{RecursiveMode, Watcher};

use crate::migrate::MigrationOffer;
use crate::writer::{added_errors, secret_lines, sha256_hex};
use crate::{
    CompanyRefusal, ConfigEdit, ConfigSnapshot, ConfigWriter, FOLDERS, Finding, LoadError,
    LoadOptions, MODEL_FILES, Severity, WriteError, checks, company_head, linediff, load_exact,
    tracked,
};

/// How long a path must be quiet before it is compared (editors save by temp
/// file and rename, and FSEvents coalesces events).
const SETTLE: Duration = Duration::from_millis(300);
const COMPANY: &str = "company.yaml";
const SECRET_MESSAGE: &str = "secret-looking value";

/// What the watcher announces.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigEvent {
    /// A tracked file changed outside the app; nothing applies until Reload.
    ChangedOnDisk {
        /// Project-relative path with forward slashes.
        file: String,
        /// sha256 of the accepted bytes; `None` for a file created outside.
        before_sha256: Option<String>,
        /// sha256 of the bytes on disk; `None` for a file deleted outside.
        after_sha256: Option<String>,
        /// Unified diff from the accepted bytes, or a worded note when the
        /// new bytes hold a secret-looking value.
        diff: String,
        /// The errors the change adds (file, line, fix); empty when valid.
        findings: Vec<Finding>,
        /// `company.yaml`'s `project_id` changed or was removed.
        identity_change: bool,
    },
    /// A change announced earlier was undone on disk (the file holds the
    /// accepted bytes again); nothing is pending for it any more.
    Withdrawn {
        /// Project-relative path with forward slashes.
        file: String,
    },
}

/// Why a Reload applied nothing.
#[derive(Debug, thiserror::Error)]
pub enum ReloadError {
    /// (A struct variant with no fields, matched as `NothingPending { .. }`.)
    #[error("nothing to reload: no config file changed on disk outside the app")]
    NothingPending {},
    #[error(
        "refused: the change on disk would leave the project invalid, so nothing was applied; fix the file and the change is announced again:\n{}",
        lines(.findings)
    )]
    Invalid { findings: Vec<Finding> },
    #[error(
        "refused: {}; the project_id ties this folder to its runtime folder, so Reload never applies a change to it. {}",
        identity_words(.before, .after),
        identity_fix(.before)
    )]
    IdentityChange {
        /// The accepted `project_id`, if any.
        before: Option<String>,
        /// The `project_id` on disk, if any.
        after: Option<String>,
    },
    #[error("the project cannot be read: {0}")]
    Project(#[from] LoadError),
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error("the reload stopped: {0}")]
    Interrupted(String),
}

/// Why a session could not open.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Load(#[from] LoadError),
    #[error("the project has errors, so no snapshot can be built:\n{}", lines(.findings))]
    Invalid { findings: Vec<Finding> },
    /// Story 1.10 (AD-7, AD-16): config files are at an older
    /// `schema_version`; nothing runs until the owner accepts the offers.
    /// `blocked` names each older file that cannot be offered yet (a
    /// secret-looking value, a link, a step's reason), with what to do.
    #[error("{}", pending_words(.offers, .blocked, .unmigratable))]
    /// `unmigratable` names each older file with no step path, as
    /// `<file> (schema_version <n>)`: they stay errors after the accept.
    MigrationPending {
        offers: Vec<MigrationOffer>,
        blocked: Vec<String>,
        unmigratable: Vec<String>,
    },
    #[error(transparent)]
    Write(#[from] WriteError),
    #[error("cannot watch the project folder: {0}")]
    Watch(String),
}

fn offer_files(offers: &[MigrationOffer]) -> String {
    let files: Vec<String> = offers
        .iter()
        .map(|o| format!("{} from schema_version {} to {}", o.file, o.from, o.to))
        .collect();
    files.join(", ")
}

fn pending_words(offers: &[MigrationOffer], blocked: &[String], unmigratable: &[String]) -> String {
    let mut words = if blocked.is_empty() {
        format!(
            "the project waits for a config migration ({}); review each diff and accept the migration before the project opens",
            offer_files(offers)
        )
    } else {
        format!(
            "the project waits for a config migration that cannot be accepted yet, because:\n- {}\n(also waiting: {})",
            blocked.join("\n- "),
            offer_files(offers)
        )
    };
    if !unmigratable.is_empty() {
        words.push_str(&format!(
            "\nthese files cannot be migrated by the app and stay errors after the accept (rewrite them by hand): {}",
            unmigratable.join(", ")
        ));
    }
    words
}

fn lines(findings: &[Finding]) -> String {
    let all: Vec<String> = findings.iter().map(Finding::to_string).collect();
    all.join("\n")
}

fn identity_words(before: &Option<String>, after: &Option<String>) -> String {
    match (before, after) {
        (Some(b), Some(a)) => format!("company.yaml's project_id changed on disk from {b} to {a}"),
        (Some(b), None) => format!("company.yaml's project_id {b} was removed on disk"),
        (None, Some(a)) => format!("a project_id {a} was added to company.yaml on disk"),
        (None, None) => "company.yaml's project_id changed on disk".into(),
    }
}

fn identity_fix(before: &Option<String>) -> String {
    match before {
        Some(b) => format!(
            "Write back `project_id: {b}` in company.yaml; the other pending changes stay waiting and apply at the next Reload"
        ),
        None => {
            "Remove the project_id line from company.yaml (the app assigns the id itself); the other pending changes stay waiting and apply at the next Reload".into()
        }
    }
}

/// One announced change waiting for Reload.
#[derive(Debug, Clone)]
struct Pending {
    /// The bytes on disk when it was announced; `None` when deleted.
    after: Option<Vec<u8>>,
    after_sha256: Option<String>,
    /// The announced diff (kept in History).
    diff: String,
    /// (accepted, on disk) `project_id` when the change alters it.
    identity: Option<(Option<String>, Option<String>)>,
}

struct State {
    /// The accepted bytes of every tracked file.
    accepted: BTreeMap<String, Vec<u8>>,
    /// Built only from `accepted`.
    snapshot: Arc<ConfigSnapshot>,
    /// Announced changes by file.
    pending: BTreeMap<String, Pending>,
}

struct Inner {
    root: PathBuf,
    opts: LoadOptions,
    writer: ConfigWriter,
    history: Arc<dyn ConfigHistory>,
    on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync>,
    state: Mutex<State>,
}

/// An open project's config: accepted bytes, the snapshot, the watcher and
/// the writer. Dropping it stops the watcher.
pub struct ConfigSession {
    inner: Arc<Inner>,
    watcher: Option<notify::RecommendedWatcher>,
    worker: Option<JoinHandle<()>>,
}

impl ConfigSession {
    /// Opens the project at `root` (it must be valid), starts the watcher and
    /// sends each announced change to `on_event` (from the watcher's thread).
    /// Loads with the default [`LoadOptions`]; see [`ConfigSession::open_in`].
    pub fn open(
        root: &Path,
        history: Arc<dyn ConfigHistory>,
        on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync>,
    ) -> Result<ConfigSession, SessionError> {
        ConfigSession::open_in(root, LoadOptions::default(), history, on_event)
    }

    /// [`ConfigSession::open`], with every snapshot loaded as `opts` says
    /// (the project's runtime `knowledge/` root, say), as [`crate::load_in`].
    pub fn open_in(
        root: &Path,
        opts: LoadOptions,
        history: Arc<dyn ConfigHistory>,
        on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync>,
    ) -> Result<ConfigSession, SessionError> {
        let writer = ConfigWriter::open(root, history.clone())?;
        // The watch starts before the first read, so no change slips between.
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(tx).map_err(|e| SessionError::Watch(e.to_string()))?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| SessionError::Watch(e.to_string()))?;
        let accepted = scan(root);
        // A pending migration builds no snapshot (AD-16): nothing runs on
        // bytes the owner has not accepted.
        let offers = crate::migrate::offers_in(root, &accepted);
        if !offers.is_empty() {
            let blocked = crate::migrate::blocked(root, &accepted);
            let unmigratable = crate::migrate::no_path(root, &accepted);
            return Err(SessionError::MigrationPending {
                offers,
                blocked,
                unmigratable,
            });
        }
        let loaded = load_exact(root, accepted.clone(), opts.clone())?;
        let Some(snapshot) = loaded.snapshot else {
            let findings = loaded
                .findings
                .into_iter()
                .filter(|f| f.severity == Severity::Error)
                .collect();
            return Err(SessionError::Invalid { findings });
        };
        let mut roots = vec![root.to_path_buf()];
        if let Ok(real) = std::fs::canonicalize(root) {
            roots.insert(0, real);
        }
        let inner = Arc::new(Inner {
            root: root.to_path_buf(),
            opts,
            writer,
            history,
            on_event,
            state: Mutex::new(State {
                accepted,
                snapshot: Arc::new(snapshot),
                pending: BTreeMap::new(),
            }),
        });
        let worker_inner = inner.clone();
        let worker = std::thread::Builder::new()
            .name("am-config-watch".into())
            .spawn(move || watch_loop(&worker_inner, &rx, &roots))
            .map_err(|e| SessionError::Watch(e.to_string()))?;
        Ok(ConfigSession {
            inner,
            watcher: Some(watcher),
            worker: Some(worker),
        })
    }

    /// The live snapshot, built only from accepted bytes.
    pub fn snapshot(&self) -> Arc<ConfigSnapshot> {
        match self.inner.state.lock() {
            Ok(st) => st.snapshot.clone(),
            Err(poisoned) => poisoned.into_inner().snapshot.clone(),
        }
    }

    /// The app's own edit through the writer; raises no event.
    pub fn apply(&self, actor: Actor, edit: ConfigEdit) -> Result<HistoryEntry, WriteError> {
        let file = edit.file.clone();
        self.own_write(Some(file), |w| w.apply(actor, edit))
    }

    /// [`ConfigWriter::undo`] through the session; raises no event.
    pub fn undo(&self, actor: Actor, id: &str) -> Result<HistoryEntry, WriteError> {
        self.own_write(self.entry_file(id), |w| w.undo(actor, id))
    }

    /// [`ConfigWriter::restore`] through the session; raises no event.
    pub fn restore(&self, actor: Actor, id: &str) -> Result<HistoryEntry, WriteError> {
        self.own_write(self.entry_file(id), |w| w.restore(actor, id))
    }

    /// The file History entry `id` names, if any.
    fn entry_file(&self, id: &str) -> Option<String> {
        self.inner
            .history
            .entry(id.to_string())
            .ok()
            .flatten()
            .map(|e| e.file)
    }

    /// Runs `write` on `file` under the session lock. Once it returns (done
    /// or not), the writer forgets its fingerprints for `file`: they cover
    /// only a write in flight, so a later outside save of the same bytes is
    /// announced.
    fn own_write(
        &self,
        file: Option<String>,
        write: impl FnOnce(&ConfigWriter) -> Result<HistoryEntry, WriteError>,
    ) -> Result<HistoryEntry, WriteError> {
        let inner = &self.inner;
        let mut st = inner
            .state
            .lock()
            .map_err(|_| WriteError::Interrupted("an earlier session call panicked".into()))?;
        // The writer reads the file from disk: a change not yet reloaded
        // would otherwise be taken in silently.
        let files = differing(&scan(&inner.root), &st.accepted);
        if !files.is_empty() {
            return Err(WriteError::ChangedOnDisk { files });
        }
        let written = write(&inner.writer);
        if let Some(file) = &file {
            inner.writer.forget_own_writes(file);
        }
        let entry = written?;
        let mut next = st.accepted.clone();
        next.insert(entry.file.clone(), entry.after.clone());
        // The writer checked the project with disk equal to the accepted
        // bytes, so this load has no error; if a racing outside change made
        // it fail, the snapshot stays until the Reload of that change.
        if let Ok(loaded) = load_exact(&inner.root, next.clone(), inner.opts.clone())
            && let Some(snapshot) = loaded.snapshot
        {
            st.snapshot = Arc::new(snapshot);
        }
        st.accepted = next;
        Ok(entry)
    }

    /// Applies every pending change as one new snapshot, recording one
    /// `outside_edit` History entry per file by `actor`. All or nothing.
    pub fn reload(&self, actor: Actor) -> Result<Vec<HistoryEntry>, ReloadError> {
        let inner = &self.inner;
        let mut st = inner
            .state
            .lock()
            .map_err(|_| ReloadError::Interrupted("an earlier session call panicked".into()))?;
        if st.pending.is_empty() {
            return Err(ReloadError::NothingPending {});
        }
        if let Some((before, after)) = st.pending.values().find_map(|p| p.identity.clone()) {
            return Err(ReloadError::IdentityChange { before, after });
        }
        let mut next = st.accepted.clone();
        for (file, p) in &st.pending {
            put(&mut next, file, p.after.clone());
        }
        let before_errors = errors(&load_exact(
            &inner.root,
            st.accepted.clone(),
            inner.opts.clone(),
        )?);
        let loaded = load_exact(&inner.root, next.clone(), inner.opts.clone())?;
        let after_errors = errors(&loaded);
        let mut findings = added_errors(before_errors, after_errors.clone());
        for (file, p) in &st.pending {
            if let Some(bytes) = &p.after {
                add_secret_findings(&mut findings, file, &secret_lines(file, bytes));
            }
        }
        if findings.is_empty() {
            // Errors the change did not add (a linked folder gone, say)
            // still leave no snapshot.
            findings = after_errors;
        }
        let snapshot = match loaded.snapshot {
            Some(s) if findings.is_empty() => s,
            _ => {
                return Err(ReloadError::Invalid {
                    findings: scrubbed(findings),
                });
            }
        };
        // History first: every intent, then every commit. A failed intent
        // drops the earlier ones; nothing is applied.
        let mut ids = Vec::new();
        for (file, p) in &st.pending {
            let before = st.accepted.get(file).cloned().unwrap_or_default();
            let after = p.after.clone().unwrap_or_default();
            let intent = NewIntent {
                file: file.clone(),
                kind: HistoryKind::OutsideEdit,
                ref_entry: None,
                before_sha256: sha256_hex(&before),
                after_sha256: sha256_hex(&after),
                before,
                after,
                diff: p.diff.clone(),
                audit: Vec::new(),
            };
            match inner.history.begin_intent(actor.clone(), intent) {
                Ok(id) => ids.push(id),
                Err(e) => {
                    for id in ids {
                        let _ = inner.history.drop_intent(actor.clone(), id);
                    }
                    return Err(e.into());
                }
            }
        }
        let mut entries = Vec::with_capacity(ids.len());
        for id in ids {
            // A commit that fails returns its error: the snapshot and the
            // pending changes stay, and entries committed before it stay in
            // History (a second Reload records those files again). Its
            // intent and the later ones stay pending in History: the writer
            // settles one before its next write to that file, and its
            // reconcile at the next start completes those whose file is a
            // config file holding the new bytes and drops the rest (a skill
            // reference file, a file deleted outside), with no entry.
            entries.push(inner.history.commit_entry(actor.clone(), id)?);
        }
        st.accepted = next;
        st.snapshot = Arc::new(snapshot);
        st.pending.clear();
        Ok(entries)
    }
}

impl Drop for ConfigSession {
    fn drop(&mut self) {
        // Dropping the watcher closes its channel, which ends the worker.
        drop(self.watcher.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

// ------------------------------------------------------------------ watcher

fn watch_loop(inner: &Inner, rx: &Receiver<notify::Result<notify::Event>>, roots: &[PathBuf]) {
    // Scope (a tracked file, a folder that holds them, or "" for all) to the
    // time of its last event.
    let mut dirty: BTreeMap<String, Instant> = BTreeMap::new();
    loop {
        let got = match dirty.values().min() {
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(t) => rx.recv_timeout((*t + SETTLE).saturating_duration_since(Instant::now())),
        };
        match got {
            Ok(Ok(event)) => {
                let now = Instant::now();
                for scope in dirty_scopes(&event, roots) {
                    dirty.insert(scope, now);
                }
            }
            // A watcher error may have lost events: compare everything.
            Ok(Err(_)) => {
                dirty.insert(String::new(), Instant::now());
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let now = Instant::now();
        let ready: Vec<String> = dirty
            .iter()
            .filter(|(_, t)| now >= **t + SETTLE)
            .map(|(k, _)| k.clone())
            .collect();
        if !ready.is_empty() {
            for k in &ready {
                dirty.remove(k);
            }
            inner.settle(&ready);
        }
    }
}

/// The scopes `event` makes the session compare: `""` (everything) for a
/// rescan notice, else each path's scope. An Access event (an open, a read,
/// a close) changes no content and marks nothing: inotify reports every
/// open, the worker's own reads included, which would otherwise wake the
/// worker every [`SETTLE`] for ever.
fn dirty_scopes(event: &notify::Event, roots: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    if event.need_rescan() {
        out.push(String::new());
    }
    if matches!(event.kind, notify::EventKind::Access(_)) {
        return out;
    }
    out.extend(
        event
            .paths
            .iter()
            .filter_map(|p| relative(roots, p).and_then(|r| scope_of(&r))),
    );
    out
}

/// `path` relative to the project root, with forward slashes; `""` for the
/// root itself.
fn relative(roots: &[PathBuf], path: &Path) -> Option<String> {
    let rest = roots.iter().find_map(|r| path.strip_prefix(r).ok())?;
    let mut parts = Vec::new();
    for c in rest.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_str()?.to_string()),
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// What an event at `rel` makes the session compare: the file itself when
/// tracked, a folder that holds tracked files (a folder renamed or removed
/// reports only itself), or nothing.
fn scope_of(rel: &str) -> Option<String> {
    let parts: Vec<&str> = rel.split('/').collect();
    let container = match parts.as_slice() {
        [""] => true,
        [d] => *d == "models" || *d == "skills" || FOLDERS.iter().any(|(f, _)| f == d),
        ["skills", _] | ["skills", _, "references"] => true,
        _ => false,
    };
    (container || tracked(rel)).then(|| rel.to_string())
}

impl Inner {
    /// Compares every tracked file in `scopes` and announces what is new.
    fn settle(&self, scopes: &[String]) {
        let events: Vec<ConfigEvent> = {
            let Ok(mut st) = self.state.lock() else {
                return;
            };
            let files = self.candidates(&st, scopes);
            files
                .iter()
                .filter_map(|file| self.examine(&mut st, file))
                .collect()
        };
        for e in events {
            (self.on_event)(e);
        }
    }

    fn candidates(&self, st: &State, scopes: &[String]) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut disk: Option<BTreeMap<String, Vec<u8>>> = None;
        for scope in scopes {
            if tracked(scope) {
                out.insert(scope.clone());
                continue;
            }
            let disk = disk.get_or_insert_with(|| scan(&self.root));
            let prefix = format!("{scope}/");
            let under = |k: &&String| scope.is_empty() || k.starts_with(&prefix);
            out.extend(disk.keys().filter(under).cloned());
            out.extend(st.accepted.keys().filter(under).cloned());
            out.extend(st.pending.keys().filter(under).cloned());
        }
        out
    }

    /// The event for `file` when its content on disk is new, recording it
    /// as pending; `Withdrawn` when it equals the accepted bytes again while
    /// a change was pending; `None` when it equals them with nothing pending,
    /// was announced already or is the writer's own write in flight.
    fn examine(&self, st: &mut State, file: &str) -> Option<ConfigEvent> {
        let disk = on_disk(&self.root, file);
        let after_sha256 = disk.as_deref().map(sha256_hex);
        let before = st.accepted.get(file);
        let before_sha256 = before.map(|b| sha256_hex(b));
        // An empty file and no file are the same content: an empty file
        // created or deleted outside is never announced or applied.
        if content(disk.as_deref()) == content(before.map(Vec::as_slice)) {
            return st.pending.remove(file).map(|_| ConfigEvent::Withdrawn {
                file: file.to_string(),
            });
        }
        if st
            .pending
            .get(file)
            .is_some_and(|p| p.after_sha256 == after_sha256)
        {
            return None;
        }
        if let Some(h) = &after_sha256
            && self.writer.is_own_write(file, h)
        {
            return None;
        }
        let before_bytes: &[u8] = before.map_or(&[], Vec::as_slice);
        let after_bytes: &[u8] = disk.as_deref().unwrap_or(&[]);
        let secrets = secret_lines(file, after_bytes);
        let diff = if secrets.is_empty() {
            linediff::unified(file, before_bytes, after_bytes)
        } else {
            secret_note(&secrets)
        };
        // The errors this change adds to the accepted bytes with every other
        // pending change in place (what a Reload would check).
        let mut base = st.accepted.clone();
        for (f, p) in &st.pending {
            if f != file {
                put(&mut base, f, p.after.clone());
            }
        }
        let mut with = base.clone();
        put(&mut with, file, disk.clone());
        let mut findings = added_errors(self.errors_of(base), self.errors_of(with));
        add_secret_findings(&mut findings, file, &secrets);
        let findings = scrubbed(findings);
        let identity = if file == COMPANY {
            identity_change(before.map(Vec::as_slice), disk.as_deref())
        } else {
            None
        };
        st.pending.insert(
            file.to_string(),
            Pending {
                after: disk,
                after_sha256: after_sha256.clone(),
                diff: diff.clone(),
                identity: identity.clone(),
            },
        );
        Some(ConfigEvent::ChangedOnDisk {
            file: file.to_string(),
            before_sha256,
            after_sha256,
            diff,
            findings,
            identity_change: identity.is_some(),
        })
    }

    fn errors_of(&self, files: BTreeMap<String, Vec<u8>>) -> Vec<Finding> {
        load_exact(&self.root, files, self.opts.clone())
            .map(|l| errors(&l))
            .unwrap_or_default()
    }
}

// ------------------------------------------------------------------ helpers

fn errors(loaded: &crate::Loaded) -> Vec<Finding> {
    loaded
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .cloned()
        .collect()
}

/// Sets (`Some`) or removes (`None`) `file` in `files`.
fn put(files: &mut BTreeMap<String, Vec<u8>>, file: &str, bytes: Option<Vec<u8>>) {
    match bytes {
        Some(b) => {
            files.insert(file.to_string(), b);
        }
        None => {
            files.remove(file);
        }
    }
}

/// The bytes of the tracked file `rel` on disk, read as the loader reads it
/// (a link is followed only for the top-level files and `models/`, which the
/// loader checks with `is_file`; folder entries are listed without following
/// links); `None` when it is absent or not a file.
fn on_disk(root: &Path, rel: &str) -> Option<Vec<u8>> {
    let path = root.join(rel);
    let follows = !rel.contains('/') || rel.starts_with("models/");
    let file = if follows {
        path.is_file()
    } else {
        std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file())
    };
    if !file {
        return None;
    }
    std::fs::read(&path).ok()
}

/// The non-`.DS_Store` names in the folder `rel`, sorted.
fn names(root: &Path, rel: &str) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(root.join(rel)) else {
        return Vec::new();
    };
    let mut out: Vec<String> = read
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != ".DS_Store")
        .collect();
    out.sort();
    out
}

/// Every tracked file on disk with its bytes.
pub(crate) fn scan(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut rels: Vec<String> = vec![COMPANY.into(), "tools.yaml".into()];
    rels.extend(MODEL_FILES.iter().map(|m| format!("models/{m}.yaml")));
    for (folder, _) in FOLDERS {
        rels.extend(
            names(root, folder)
                .into_iter()
                .map(|n| format!("{folder}/{n}")),
        );
    }
    for id in names(root, "skills") {
        rels.push(format!("skills/{id}/SKILL.md"));
        let refs = format!("skills/{id}/references");
        rels.extend(
            names(root, &refs)
                .into_iter()
                .map(|n| format!("{refs}/{n}")),
        );
    }
    rels.into_iter()
        .filter(|r| tracked(r))
        .filter_map(|r| on_disk(root, &r).map(|b| (r, b)))
        .collect()
}

/// A file's content for comparing disk with the accepted bytes: an empty
/// file is the same as no file.
fn content(bytes: Option<&[u8]>) -> Option<&[u8]> {
    bytes.filter(|b| !b.is_empty())
}

/// The files whose content differs between `disk` and `accepted` (an empty
/// file is the same as no file), sorted.
fn differing(
    disk: &BTreeMap<String, Vec<u8>>,
    accepted: &BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    let keys: BTreeSet<&String> = disk.keys().chain(accepted.keys()).collect();
    keys.into_iter()
        .filter(|k| {
            content(disk.get(*k).map(Vec::as_slice)) != content(accepted.get(*k).map(Vec::as_slice))
        })
        .cloned()
        .collect()
}

/// The note that replaces a diff whose new bytes hold a secret-looking value.
fn secret_note(lines: &[u32]) -> String {
    let at: Vec<String> = lines.iter().map(|l| format!("line {l}")).collect();
    format!(
        "The diff is not shown: the new content holds a secret-looking value ({}). Remove it by hand (keys live in the Keychain; reference an Account instead); until then the change is invalid and Reload refuses it.",
        at.join(", ")
    )
}

/// One error per secret line of `file` that no finding names yet.
fn add_secret_findings(findings: &mut Vec<Finding>, file: &str, lines: &[u32]) {
    for &line in lines {
        if !findings.iter().any(|f| f.file == file && f.line == line) {
            findings.push(Finding {
                file: file.to_string(),
                line,
                severity: Severity::Error,
                message: SECRET_MESSAGE.into(),
                fix: checks::SECRET_FIX.into(),
            });
        }
    }
}

/// `findings` sorted, with any finding that quotes a secret-looking value
/// reworded so the value never reaches an event or an error.
fn scrubbed(mut findings: Vec<Finding>) -> Vec<Finding> {
    for f in &mut findings {
        if checks::has_secret_prefix(&f.message) || checks::has_secret_prefix(&f.fix) {
            f.message = SECRET_MESSAGE.into();
            f.fix = checks::SECRET_FIX.into();
        }
    }
    findings.sort_by(|a, b| {
        (&a.file, a.line, a.severity, &a.message).cmp(&(&b.file, b.line, b.severity, &b.message))
    });
    findings.dedup();
    findings
}

/// `company.yaml`'s `project_id` in `bytes`: `Some(None)` when there is none
/// (or no file), `None` when the file cannot be read as YAML (its findings
/// say so). A secret-looking value is not repeated.
fn project_id(bytes: Option<&[u8]>) -> Option<Option<String>> {
    let Some(bytes) = bytes else {
        return Some(None);
    };
    let text = std::str::from_utf8(bytes).ok()?;
    let value = match company_head(text) {
        Ok(head) => head.project_id.map(|id| id.to_string()),
        Err(CompanyRefusal::BadProjectId { value, .. }) => Some(value),
        Err(_) => return None,
    };
    Some(value.map(|v| {
        if checks::has_secret_prefix(&v) {
            "(a secret-looking value)".to_string()
        } else {
            v
        }
    }))
}

/// (accepted, on disk) `project_id` when they differ.
fn identity_change(
    before: Option<&[u8]>,
    after: Option<&[u8]>,
) -> Option<(Option<String>, Option<String>)> {
    match (project_id(before), project_id(after)) {
        (Some(b), Some(a)) if a != b => Some((b, a)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use notify::event::{AccessKind, AccessMode, DataChange, Flag, ModifyKind};
    use notify::{Event, EventKind};

    use super::dirty_scopes;

    fn roots() -> Vec<PathBuf> {
        vec![PathBuf::from("/project")]
    }

    fn at(kind: EventKind) -> Event {
        Event::new(kind).add_path(PathBuf::from("/project/seats/a.md"))
    }

    /// inotify reports every open (the worker's own reads too) as an Access
    /// event; it changes no content, so it marks nothing dirty.
    #[test]
    fn access_events_mark_nothing_dirty() {
        for kind in [
            EventKind::Access(AccessKind::Open(AccessMode::Any)),
            EventKind::Access(AccessKind::Read),
            EventKind::Access(AccessKind::Close(AccessMode::Read)),
        ] {
            assert!(dirty_scopes(&at(kind), &roots()).is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn content_events_and_rescans_mark_their_scope() {
        let modify = at(EventKind::Modify(ModifyKind::Data(DataChange::Any)));
        assert_eq!(dirty_scopes(&modify, &roots()), ["seats/a.md"]);
        let close_write = at(EventKind::Access(AccessKind::Close(AccessMode::Write)));
        assert!(dirty_scopes(&close_write, &roots()).is_empty());
        let rescan = Event::new(EventKind::Other).set_flag(Flag::Rescan);
        assert_eq!(dirty_scopes(&rescan, &roots()), [""]);
        let other = at(EventKind::Any);
        assert_eq!(dirty_scopes(&other, &roots()), ["seats/a.md"]);
    }
}
