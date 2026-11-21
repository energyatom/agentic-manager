//! Moving a legacy in-project runtime folder out of the project (AD-32,
//! story 1.13; owner decision 2026-11-21: kept, never deleted).
//!
//! An older binary kept its runtime state in `<project>/.agentic/`. On the
//! first open by this binary, before the mint and before any store opens,
//! [`move_legacy`] fills the project's runtime folder from it:
//!
//! 1. The journal (`RuntimeLayout::legacy_journal`) is written first, with
//!    the source and the sha256 manifest of every legacy file (phase
//!    `copying`). The legacy lock is try-locked without rewriting the file
//!    and held for the whole move; a live holder refuses the move.
//! 2. `state.db` is copied with `VACUUM INTO` (through a read-only
//!    connection) and becomes the runtime store; every other file is
//!    stream-copied ([`crate::stream_copy`], never `std::fs::copy`) and its
//!    copy checked against the manifest. `lock`, `.gitignore`,
//!    `state.db-wal` and `state.db-shm` are not copied; their bytes stay in
//!    the kept folder. A copy that does not match refuses the move and
//!    leaves the old folder untouched (phase `copied` once all match).
//! 3. A free `legacy-agentic-<YYYY-MM-DD>[-n]` name (UTC) is chosen (the
//!    caller holds the runtime base's lock) and written to the journal
//!    (phase `moving`); the old folder leaves the project by a rename, or,
//!    when the rename fails or the project lies under a cloud root, by a
//!    stream copy checked against the manifest after which the project's
//!    copy is removed. The kept folder gets 0700 and its files 0600.
//! 4. The caller writes a minted id (`id-written`) and, once the store is
//!    open and the move audited and reported, ends the journal (`done`) with
//!    [`mark`]; until then an open resumes (and reports) the move.
//!
//! An open that finds an unfinished journal resumes from its phase: before
//! the rename the source is still the truth, so the manifest is rebuilt and
//! the copy made again (nothing in the runtime folder is trusted); after
//! it, the move is finished from the recorded name. The app never deletes
//! the kept folder.

use std::fs::{File, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

use am_domain::RuntimeLayout;
use am_domain::runtime::legacy_dir;
use rusqlite::params;

use crate::{StoreError, fsx, project};

const MAGIC: &str = "am-legacy-move 1";

/// A runtime file's name as `RuntimeLayout` names it (the legacy folder has
/// the same layout as a runtime folder's top level).
fn name_of(path: PathBuf) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `state.db`, relative to a runtime (or legacy) folder.
fn database() -> String {
    name_of(RuntimeLayout::at(PathBuf::new()).state_db())
}

/// Legacy files that are not copied into the runtime folder (the lock, the
/// `.gitignore` and the database's side files); their bytes stay in the
/// kept folder.
fn skipped(path: &str) -> bool {
    let db = database();
    path == name_of(RuntimeLayout::at(PathBuf::new()).lock())
        || path == ".gitignore"
        || path == format!("{db}-wal")
        || path == format!("{db}-shm")
}

/// Not compared after a copy of the kept folder: SQLite may rewrite its
/// shared-memory index while the database is read.
fn unstable(path: &str) -> bool {
    path == format!("{}-shm", database())
}

/// One move, as the resolver asks for it.
#[derive(Debug, Clone)]
pub struct LegacyMove {
    pub project_root: PathBuf,
    /// The runtime folder (it exists, 0700, and holds the root record).
    pub layout: RuntimeLayout,
    /// The project id (minted in memory when `company.yaml` has none).
    pub project_id: String,
    pub id_minted: bool,
    /// Always take the copy path (the project lies under a cloud root;
    /// am-app decides).
    pub copy_always: bool,
}

/// What a finished move left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyMoved {
    /// The kept folder inside the runtime folder.
    pub kept: PathBuf,
    /// The legacy folder held a `state.db`, now the runtime store.
    pub had_database: bool,
}

/// A journal's phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Copying,
    Copied,
    Moving,
    IdWritten,
    Done,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::Copying => "copying",
            Phase::Copied => "copied",
            Phase::Moving => "moving",
            Phase::IdWritten => "id-written",
            Phase::Done => "done",
        }
    }

    fn parse(text: &str) -> Option<Phase> {
        Some(match text {
            "copying" => Phase::Copying,
            "copied" => Phase::Copied,
            "moving" => Phase::Moving,
            "id-written" => Phase::IdWritten,
            "done" => Phase::Done,
            _ => return None,
        })
    }
}

/// One legacy file: its path relative to the legacy folder and its sha256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub path: String,
    pub sha256: String,
}

/// The move journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journal {
    /// The legacy folder being moved.
    pub source: PathBuf,
    pub project_id: String,
    pub id_minted: bool,
    pub phase: Phase,
    /// The kept folder's name, once chosen.
    pub kept: Option<String>,
    pub manifest: Vec<ManifestEntry>,
}

impl Journal {
    pub fn unfinished(&self) -> bool {
        self.phase != Phase::Done
    }

    /// The legacy folder held a database.
    pub fn had_database(&self) -> bool {
        self.manifest.iter().any(|e| e.path == database())
    }

    fn to_text(&self) -> String {
        let mut t = format!(
            "{MAGIC}\nsource: {}\nproject_id: {}\nid_minted: {}\nphase: {}\n",
            hex::encode(self.source.as_os_str().as_encoded_bytes()),
            self.project_id,
            self.id_minted,
            self.phase.as_str()
        );
        if let Some(k) = &self.kept {
            t.push_str(&format!("kept: {}\n", hex::encode(k.as_bytes())));
        }
        for e in &self.manifest {
            t.push_str(&format!(
                "file: {} {}\n",
                e.sha256,
                hex::encode(e.path.as_bytes())
            ));
        }
        t
    }

    fn parse(text: &str) -> Option<Journal> {
        let mut lines = text.lines();
        if lines.next()? != MAGIC {
            return None;
        }
        let text_of = |h: &str| String::from_utf8(hex::decode(h).ok()?).ok();
        let mut source = None;
        let mut project_id = None;
        let mut id_minted = None;
        let mut phase = None;
        let mut kept = None;
        let mut manifest = Vec::new();
        for line in lines {
            let (key, value) = line.split_once(": ")?;
            match key {
                "source" => source = Some(PathBuf::from(text_of(value)?)),
                "project_id" => project_id = Some(value.to_string()),
                "id_minted" => id_minted = Some(value == "true"),
                "phase" => phase = Phase::parse(value),
                "kept" => kept = Some(text_of(value)?),
                "file" => {
                    let (sha, path) = value.split_once(' ')?;
                    manifest.push(ManifestEntry {
                        path: text_of(path)?,
                        sha256: sha.to_string(),
                    });
                }
                _ => return None,
            }
        }
        Some(Journal {
            source: source?,
            project_id: project_id?,
            id_minted: id_minted?,
            phase: phase?,
            kept,
            manifest,
        })
    }
}

fn io(context: String) -> impl FnOnce(std::io::Error) -> StoreError {
    move |e| StoreError::io(context, e)
}

/// The runtime folder's move journal, if one was ever written.
pub fn read_journal(layout: &RuntimeLayout) -> Result<Option<Journal>, StoreError> {
    let path = layout.legacy_journal();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(StoreError::io(format!("cannot read {}", path.display()), e)),
    };
    Journal::parse(&text).map(Some).ok_or_else(|| {
        StoreError::Invalid(format!(
            "the move journal {} cannot be read; move it aside only if the project's old runtime folder was already moved, then open the project again",
            path.display()
        ))
    })
}

/// Writes the journal by temp file and rename (owner-only).
fn write_journal(layout: &RuntimeLayout, j: &Journal) -> Result<(), StoreError> {
    let path = layout.legacy_journal();
    let tmp = layout.dir().join(".legacy-move.journal.tmp");
    let mut f =
        fsx::rewrite_private_file(&tmp).map_err(io(format!("cannot write {}", tmp.display())))?;
    f.write_all(j.to_text().as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(io(format!("cannot write {}", tmp.display())))?;
    std::fs::rename(&tmp, &path).map_err(io(format!("cannot write {}", path.display())))?;
    File::open(layout.dir())
        .and_then(|d| d.sync_all())
        .map_err(io(format!("cannot sync {}", layout.dir().display())))
}

/// Sets the journal's phase (`IdWritten` after the caller wrote a minted
/// id; `Done` when the open is reported).
pub fn mark(layout: &RuntimeLayout, phase: Phase) -> Result<(), StoreError> {
    let mut j = read_journal(layout)?.ok_or_else(|| {
        StoreError::Invalid(format!(
            "there is no move journal in {}",
            layout.dir().display()
        ))
    })?;
    j.phase = phase;
    write_journal(layout, &j)
}

/// The held legacy lock (released when dropped, or when the process dies).
struct HeldLock(Option<File>);

impl Drop for HeldLock {
    fn drop(&mut self) {
        if let Some(f) = &self.0 {
            let _ = f.unlock();
        }
    }
}

/// Try-locks `<legacy>/lock` without rewriting it; a live holder refuses.
fn take_lock(legacy: &Path) -> Result<HeldLock, StoreError> {
    let path = RuntimeLayout::at(legacy.to_path_buf()).lock();
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HeldLock(None)),
        Err(e) => return Err(StoreError::io(format!("cannot open {}", path.display()), e)),
    };
    match file.try_lock() {
        Ok(()) => Ok(HeldLock(Some(file))),
        Err(TryLockError::WouldBlock) => Err(StoreError::LegacyLocked {
            dir: legacy.to_path_buf(),
            holder: project::read_holder(&path),
        }),
        Err(TryLockError::Error(e)) => {
            Err(StoreError::io(format!("cannot lock {}", path.display()), e))
        }
    }
}

/// Refuses while a live process holds the legacy folder's lock; the lock
/// file is never rewritten (the resolver's check before any write).
pub fn check_lock(legacy: &Path) -> Result<(), StoreError> {
    take_lock(legacy).map(drop)
}

/// Moves (or resumes moving) the project's legacy folder into the runtime
/// folder; see the module documentation for the phases.
pub fn move_legacy(m: &LegacyMove) -> Result<LegacyMoved, StoreError> {
    let old = legacy_dir(&m.project_root);
    let _lock = if old.is_dir() {
        take_lock(&old)?
    } else {
        HeldLock(None)
    };
    let mut j = match read_journal(&m.layout)? {
        Some(j) => j,
        None => {
            if !old.is_dir() {
                return Err(StoreError::Invalid(format!(
                    "there is no old runtime folder at {} to move",
                    old.display()
                )));
            }
            let j = Journal {
                source: old.clone(),
                project_id: m.project_id.clone(),
                id_minted: m.id_minted,
                phase: Phase::Copying,
                kept: None,
                manifest: manifest_of(&old)?,
            };
            write_journal(&m.layout, &j)?;
            j
        }
    };
    if matches!(j.phase, Phase::Copying | Phase::Copied) {
        if !old.is_dir() {
            return Err(StoreError::Invalid(format!(
                "the move journal in {} says the old runtime folder {} is still being copied, but it is gone; put it back and open the project again",
                m.layout.dir().display(),
                old.display()
            )));
        }
        // Before the rename the source is the truth: the manifest is
        // rebuilt and the copy made again.
        j.manifest = manifest_of(&old)?;
        j.phase = Phase::Copying;
        write_journal(&m.layout, &j)?;
        copy_into_runtime(&old, &m.layout, &j.manifest)?;
        j.phase = Phase::Copied;
        write_journal(&m.layout, &j)?;
        #[cfg(feature = "test-support")]
        if testing::failing(FailPoint::AfterCopy) {
            return Err(StoreError::Invalid("simulated crash after the copy".into()));
        }
        j.kept = Some(free_name(&m.layout)?);
        j.phase = Phase::Moving;
        write_journal(&m.layout, &j)?;
    }
    let name = j.kept.clone().ok_or_else(|| {
        StoreError::Invalid(format!(
            "the move journal in {} names no kept folder",
            m.layout.dir().display()
        ))
    })?;
    let kept = m.layout.dir().join(&name);
    if j.phase == Phase::Moving {
        move_out(&old, &kept, &j.manifest, m.copy_always)?;
        #[cfg(feature = "test-support")]
        if testing::failing(FailPoint::AfterRename) {
            return Err(StoreError::Invalid(
                "simulated crash after the rename".into(),
            ));
        }
    }
    Ok(LegacyMoved {
        kept,
        had_database: j.had_database(),
    })
}

/// The first free `legacy-agentic-<today>[-n]` name in the runtime folder.
fn free_name(layout: &RuntimeLayout) -> Result<String, StoreError> {
    let date = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    for n in 1..10_000 {
        let path = layout.legacy_copy(&date, n);
        if std::fs::symlink_metadata(&path).is_err() {
            return Ok(path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default());
        }
    }
    Err(StoreError::Invalid(format!(
        "no free name for the kept folder in {}",
        layout.dir().display()
    )))
}

/// Every regular file under `dir`, relative with `/`, sorted.
fn files_under(dir: &Path) -> Result<Vec<String>, StoreError> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), StoreError> {
        let read = std::fs::read_dir(dir).map_err(io(format!("cannot read {}", dir.display())))?;
        for e in read {
            let e = e.map_err(io(format!("cannot read {}", dir.display())))?;
            let kind = e
                .file_type()
                .map_err(io(format!("cannot read {}", e.path().display())))?;
            if kind.is_dir() {
                walk(base, &e.path(), out)?;
            } else if kind.is_file() {
                let rel = e.path();
                let rel = rel.strip_prefix(base).unwrap_or(&rel);
                let parts: Vec<String> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push(parts.join("/"));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

fn manifest_of(dir: &Path) -> Result<Vec<ManifestEntry>, StoreError> {
    files_under(dir)?
        .into_iter()
        .map(|path| {
            let full = dir.join(&path);
            let sha256 =
                fsx::sha256_file(&full).map_err(io(format!("cannot read {}", full.display())))?;
            Ok(ManifestEntry { path, sha256 })
        })
        .collect()
}

/// Creates every missing folder from `base` down to `dir` with mode 0700.
fn private_dirs(base: &Path, dir: &Path) -> Result<(), StoreError> {
    let rel = dir.strip_prefix(base).unwrap_or(dir);
    let mut at = base.to_path_buf();
    for c in rel.components() {
        at.push(c);
        fsx::ensure_private_dir(&at).map_err(io(format!("cannot create {}", at.display())))?;
    }
    Ok(())
}

/// A runtime-folder name a legacy file may not take.
fn reserved(layout: &RuntimeLayout, path: &str) -> bool {
    let first = path.split('/').next().unwrap_or("");
    let own = |p: PathBuf| p.file_name().is_some_and(|n| n.to_string_lossy() == first);
    own(layout.root_record())
        || own(layout.legacy_journal())
        || RuntimeLayout::is_kept_legacy(first)
}

/// Copies the legacy files the runtime folder takes, each checked.
fn copy_into_runtime(
    old: &Path,
    layout: &RuntimeLayout,
    manifest: &[ManifestEntry],
) -> Result<(), StoreError> {
    for e in manifest {
        if skipped(&e.path) || reserved(layout, &e.path) {
            continue;
        }
        if e.path == database() {
            vacuum_into(
                &RuntimeLayout::at(old.to_path_buf()).state_db(),
                &layout.state_db(),
            )?;
            continue;
        }
        let src = old.join(&e.path);
        let dst = layout.dir().join(&e.path);
        if let Some(parent) = dst.parent() {
            private_dirs(layout.dir(), parent)?;
        }
        copy_checked(old, &src, &dst, e)?;
    }
    Ok(())
}

/// Stream-copies one file and checks the written bytes against the manifest.
fn copy_checked(old: &Path, src: &Path, dst: &Path, e: &ManifestEntry) -> Result<(), StoreError> {
    fsx::stream_copy(src, dst).map_err(io(format!(
        "cannot copy {} to {}",
        src.display(),
        dst.display()
    )))?;
    #[cfg(feature = "test-support")]
    if testing::failing(FailPoint::CorruptCopy) {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(dst)
            .map_err(io(format!("cannot open {}", dst.display())))?;
        f.write_all(b"!")
            .map_err(io(format!("cannot write {}", dst.display())))?;
    }
    let got = fsx::sha256_file(dst).map_err(io(format!("cannot read {}", dst.display())))?;
    if got != e.sha256 && !unstable(&e.path) {
        return Err(StoreError::LegacyUnverified {
            dir: old.to_path_buf(),
            file: e.path.clone(),
        });
    }
    Ok(())
}

/// `VACUUM INTO` the runtime store from the legacy database, read through a
/// read-only connection; an earlier partial copy is replaced.
fn vacuum_into(src: &Path, dst: &Path) -> Result<(), StoreError> {
    match std::fs::remove_file(dst) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(StoreError::io(
                format!("cannot remove {}", dst.display()),
                e,
            ));
        }
    }
    fsx::create_private_file(dst).map_err(io(format!("cannot create {}", dst.display())))?;
    let target = dst
        .to_str()
        .ok_or_else(|| StoreError::Invalid(format!("{} is not UTF-8", dst.display())))?;
    let result = crate::open_read_only(src)
        .and_then(|c| c.execute("VACUUM INTO ?1", params![target]).map(drop));
    if let Err(e) = result {
        let _ = std::fs::remove_file(dst);
        return Err(e.into());
    }
    Ok(())
}

/// The old folder leaves the project as `kept`: by a rename, or by a copy
/// checked against the manifest after which the project's copy is removed.
fn move_out(
    old: &Path,
    kept: &Path,
    manifest: &[ManifestEntry],
    copy_always: bool,
) -> Result<(), StoreError> {
    if !old.is_dir() {
        if kept.is_dir() {
            return Ok(());
        }
        return Err(StoreError::Invalid(format!(
            "the old runtime folder {} and its kept copy {} are both gone",
            old.display(),
            kept.display()
        )));
    }
    if !copy_always && std::fs::rename(old, kept).is_ok() {
        return private_tree(kept);
    }
    private_dirs(kept.parent().unwrap_or(kept), kept)?;
    for e in manifest {
        let src = old.join(&e.path);
        let dst = kept.join(&e.path);
        if let Some(parent) = dst.parent() {
            private_dirs(kept, parent)?;
        }
        copy_checked(old, &src, &dst, e)?;
    }
    // Files that appeared since the manifest (SQLite side files) go too.
    for path in files_under(old)? {
        if !manifest.iter().any(|e| e.path == path) {
            let dst = kept.join(&path);
            if let Some(parent) = dst.parent() {
                private_dirs(kept, parent)?;
            }
            fsx::stream_copy(&old.join(&path), &dst)
                .map_err(io(format!("cannot copy {}", old.join(&path).display())))?;
        }
    }
    #[cfg(feature = "test-support")]
    testing::note_kept_copy();
    std::fs::remove_dir_all(old).map_err(io(format!("cannot remove {}", old.display())))
}

/// Folders 0700 and files 0600 under (and including) `dir`.
fn private_tree(dir: &Path) -> Result<(), StoreError> {
    fsx::make_private(dir, true)
        .map_err(io(format!("cannot set the mode of {}", dir.display())))?;
    let read = std::fs::read_dir(dir).map_err(io(format!("cannot read {}", dir.display())))?;
    for e in read.flatten() {
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            private_tree(&e.path())?;
        } else if kind.is_file() {
            fsx::make_private(&e.path(), false)
                .map_err(io(format!("cannot set the mode of {}", e.path().display())))?;
        }
    }
    Ok(())
}

/// Where a test stops or spoils a move (`test-support` only).
#[cfg(feature = "test-support")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailPoint {
    /// After every file was copied and checked, before the name is chosen.
    AfterCopy,
    /// After the old folder left the project, before the id is written.
    AfterRename,
    /// Each copied file is changed before it is checked.
    CorruptCopy,
}

/// Fail points and probes for tests (`test-support` only; one process per
/// test under nextest).
#[cfg(feature = "test-support")]
pub mod testing {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::FailPoint;

    static FAIL: Mutex<Option<FailPoint>> = Mutex::new(None);
    static KEPT_COPY: AtomicBool = AtomicBool::new(false);

    /// Stops or spoils the next moves at `point`.
    pub fn fail_at(point: Option<FailPoint>) {
        if let Ok(mut f) = FAIL.lock() {
            *f = point;
        }
    }

    pub(super) fn failing(point: FailPoint) -> bool {
        FAIL.lock().is_ok_and(|f| *f == Some(point))
    }

    pub(super) fn note_kept_copy() {
        KEPT_COPY.store(true, Ordering::SeqCst);
    }

    /// True once a move took the copy path for the kept folder.
    pub fn copied_kept_folder() -> bool {
        KEPT_COPY.load(Ordering::SeqCst)
    }
}
