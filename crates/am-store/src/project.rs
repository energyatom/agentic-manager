//! Opening a project's runtime folder (AD-32, AD-6, AD-7): the exclusive
//! project lock and the migrated store. Since story 1.13 the folder lies
//! outside the project, at `<runtime base>/<project-id>/`; `am-app` resolves
//! it and am-store opens the folder it is given ([`RuntimeLayout`]). It
//! never computes the base and never reads HOME.
//!
//! - The runtime folder is created 0700 when missing (its parent must
//!   exist). An existing folder, or a kept `legacy-agentic-*` folder inside
//!   it, with any group or other permission bit is refused, naming the fix
//!   (the kept folders are exempt from repair, not from the check).
//! - `lock` (0600) is held with the OS file lock by the store's writer
//!   thread, so it is released exactly when the read-write connection
//!   closes (the last [`Store`] or `Writer` handle dropped); it records the
//!   holder (pid, host, command, start time). The host is the one the caller
//!   gives: the `ProjectIdentity` port's Mac name, never a fallback.
//!   A second open is refused, naming that holder. The OS drops the lock when
//!   its holder dies, so a file left by a killed process never blocks; the
//!   next holder rewrites it.
//! - `state.db` (0600) is opened through [`Store`], which backs up (to
//!   `backups/pre-migration/`, the three newest kept) and migrates it and
//!   returns the [`OpenReport`].
//!
//! Nothing outside the runtime folder is written.

use std::fmt;
use std::fs::{File, TryLockError};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::time::Duration;

use am_domain::RuntimeLayout;

use crate::migrations::OpenReport;
use crate::{Store, StoreError, fsx, now_ts};

/// Who holds a project's lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockHolder {
    pub pid: u32,
    pub host: String,
    pub command: String,
    /// UTC RFC 3339 with ms and `Z`.
    pub started_at: String,
}

impl fmt::Display for LockHolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "pid {} on {}, running `{}`, started {}",
            self.pid, self.host, self.command, self.started_at
        )
    }
}

impl LockHolder {
    fn current(host: String) -> LockHolder {
        let command = std::env::args_os()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
            .replace(['\n', '\r'], " ");
        LockHolder {
            pid: std::process::id(),
            host: host.replace(['\n', '\r'], " "),
            command,
            started_at: now_ts(),
        }
    }

    fn to_text(&self) -> String {
        format!(
            "pid: {}\nhost: {}\ncommand: {}\nstarted: {}\n",
            self.pid, self.host, self.command, self.started_at
        )
    }

    fn parse(text: &str) -> Option<LockHolder> {
        let field = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(key)?.strip_prefix(": "))
                .map(str::to_string)
        };
        Some(LockHolder {
            pid: field("pid")?.parse().ok()?,
            host: field("host")?,
            command: field("command")?,
            started_at: field("started")?,
        })
    }
}

/// Words for a refusal: the holder, or that it could not be read.
pub(crate) fn holder_words(holder: &Option<LockHolder>) -> String {
    match holder {
        Some(h) => h.to_string(),
        None => "its holder could not be read".to_string(),
    }
}

/// The exclusive lock on a project; owned by the writer thread and released
/// when it drops (or when the process dies).
#[derive(Debug)]
pub(crate) struct ProjectLock {
    file: File,
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        // The file stays: removing it could race a process that has it open.
        let _ = self.file.unlock();
    }
}

/// Opens the runtime folder `layout`: prepares the folder, takes the lock
/// (recording `host`, the identity port's Mac name) and opens (backing up
/// and migrating) its `state.db`. The lock lives as long as the store's
/// read-write connection.
pub fn open_runtime(
    layout: &RuntimeLayout,
    host: String,
) -> Result<(Store, OpenReport), StoreError> {
    if !cfg!(unix) {
        return Err(StoreError::Unsupported);
    }
    prepare_dir(layout)?;
    let lock = acquire(layout, host)?;
    Store::open_reporting(
        &layout.state_db(),
        &layout.backups_pre_migration(),
        Some(lock),
    )
}

/// The runtime folder exists (0700) or is created; it, and every kept
/// legacy folder in it, has no group or other permission bit.
fn prepare_dir(layout: &RuntimeLayout) -> Result<(), StoreError> {
    let dir = layout.dir();
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.is_dir() => check_private(dir, &m)?,
        Ok(_) => {
            return Err(StoreError::Invalid(format!(
                "{} is not a folder (a file or a link); move it aside and open the project again",
                dir.display()
            )));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fsx::ensure_private_dir(dir)
                .map_err(|e| StoreError::io(format!("cannot create {}", dir.display()), e))?;
        }
        Err(e) => {
            return Err(StoreError::io(format!("cannot read {}", dir.display()), e));
        }
    }
    let entries = std::fs::read_dir(dir)
        .map_err(|e| StoreError::io(format!("cannot read {}", dir.display()), e))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if RuntimeLayout::is_kept_legacy(&name)
            && let Ok(m) = std::fs::symlink_metadata(entry.path())
            && m.is_dir()
        {
            check_private(&entry.path(), &m)?;
        }
    }
    Ok(())
}

fn check_private(dir: &Path, m: &std::fs::Metadata) -> Result<(), StoreError> {
    if let Some(mode) = fsx::mode_bits(m)
        && mode & 0o077 != 0
    {
        return Err(StoreError::LoosePermissions {
            path: dir.to_path_buf(),
            mode,
        });
    }
    Ok(())
}

fn acquire(layout: &RuntimeLayout, host: String) -> Result<ProjectLock, StoreError> {
    let path = layout.lock();
    let io_err = |what: &str, e| StoreError::io(format!("cannot {what} {}", path.display()), e);
    // The record is built before the lock is taken and written right after,
    // so a refused opener during a stale-lock takeover can read the previous
    // (dead) holder's record only for the length of that write.
    let holder = LockHolder::current(host);
    let mut file = fsx::open_private_file(&path).map_err(|e| io_err("open", e))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            let holder = read_holder(&path);
            return Err(StoreError::Locked { lock: path, holder });
        }
        Err(TryLockError::Error(e)) => return Err(io_err("lock", e)),
    }
    file.set_len(0).map_err(|e| io_err("write", e))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|e| io_err("write", e))?;
    file.write_all(holder.to_text().as_bytes())
        .map_err(|e| io_err("write", e))?;
    file.sync_all().map_err(|e| io_err("write", e))?;
    Ok(ProjectLock { file })
}

/// The holder recorded in a runtime folder's lock file, read without taking
/// the lock (for a refusal's words); `None` when there is none.
pub fn lock_holder(layout: &RuntimeLayout) -> Option<LockHolder> {
    std::fs::read_to_string(layout.lock())
        .ok()
        .and_then(|t| LockHolder::parse(&t))
}

/// Whether a live process holds the runtime folder's store lock, checked by
/// trying a shared lock on a fresh handle that is released at once; the
/// file is neither created nor rewritten. The OS drops a dead holder's lock,
/// so this tells a left-over marker from an open project (a reused pid
/// holds nothing).
pub fn lock_is_held(layout: &RuntimeLayout) -> bool {
    match File::open(layout.lock()) {
        Ok(f) => matches!(f.try_lock_shared(), Err(TryLockError::WouldBlock)),
        Err(_) => false,
    }
}

/// The holder writes its record right after taking the lock; a refusal that
/// lands in between waits briefly for it.
pub(crate) fn read_holder(path: &Path) -> Option<LockHolder> {
    for _ in 0..20 {
        if let Some(h) = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| LockHolder::parse(&t))
        {
            return Some(h);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holder_record_round_trips() {
        let h = LockHolder {
            pid: 42,
            host: "box".into(),
            command: "agentic serve --project /p".into(),
            started_at: "2026-11-20T10:00:00.000Z".into(),
        };
        assert_eq!(LockHolder::parse(&h.to_text()), Some(h.clone()));
        assert_eq!(
            h.to_string(),
            "pid 42 on box, running `agentic serve --project /p`, started 2026-11-20T10:00:00.000Z"
        );
        assert_eq!(LockHolder::parse(""), None);
    }

    #[test]
    fn current_holder_records_the_given_host_on_one_line() {
        let h = LockHolder::current("Office\nMac".into());
        assert_eq!(h.pid, std::process::id());
        assert_eq!(h.host, "Office Mac");
    }
}
