//! Opening a project (AD-32, AD-6, AD-7): its runtime folder `.agentic/`,
//! the exclusive project lock and the migrated store.
//!
//! - `.agentic/` is created 0700 and holds a `.gitignore` of `*`, so git
//!   ignores it without the app touching a project file. An existing folder
//!   with any group or other permission bit is refused, naming the fix.
//! - `.agentic/lock` (0600) is held with the OS file lock by the store's
//!   writer thread, so it is released exactly when the read-write connection
//!   closes (the last [`Store`] or `Writer` handle dropped); it records the
//!   holder (pid, host, command, start time).
//!   A second open is refused, naming that holder. The OS drops the lock when
//!   its holder dies, so a file left by a killed process never blocks; the
//!   next holder rewrites it.
//! - `.agentic/state.db` (0600) is opened through [`Store`], which backs up
//!   and migrates it and returns the [`OpenReport`].
//!
//! Nothing outside `.agentic/` is written.

use std::fmt;
use std::fs::{File, TryLockError};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::migrations::OpenReport;
use crate::{Store, StoreError, fsx, now_ts};

/// The runtime folder inside a project.
pub const AGENTIC_DIR: &str = ".agentic";

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
    fn current() -> LockHolder {
        let command = std::env::args_os()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
            .replace(['\n', '\r'], " ");
        LockHolder {
            pid: std::process::id(),
            host: host_name(),
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

/// The machine's name, for the lock record; never fails.
fn host_name() -> String {
    let clean = |s: &str| {
        let s = s.trim();
        (!s.is_empty()).then(|| s.replace(['\n', '\r'], " "))
    };
    if let Some(h) = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .and_then(|s| clean(&s))
    {
        return h;
    }
    if let Some(h) = std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| clean(&String::from_utf8_lossy(&o.stdout)))
    {
        return h;
    }
    std::env::var("HOSTNAME")
        .ok()
        .and_then(|s| clean(&s))
        .unwrap_or_else(|| "an unknown host".to_string())
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

/// Opens the project at `root`: prepares `.agentic/`, takes the lock and
/// opens (backing up and migrating) `.agentic/state.db`. The lock lives as
/// long as the store's read-write connection.
pub fn open_project(root: &Path) -> Result<(Store, OpenReport), StoreError> {
    if !cfg!(unix) {
        return Err(StoreError::Unsupported);
    }
    let dir = prepare_dir(root)?;
    let lock = acquire(&dir)?;
    Store::open_reporting(&dir.join("state.db"), Some(lock))
}

fn prepare_dir(root: &Path) -> Result<PathBuf, StoreError> {
    match std::fs::metadata(root) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => {
            return Err(StoreError::Invalid(format!(
                "{} is not a folder; give the project folder",
                root.display()
            )));
        }
        Err(_) => {
            return Err(StoreError::Invalid(format!(
                "the project folder {} does not exist",
                root.display()
            )));
        }
    }
    let dir = root.join(AGENTIC_DIR);
    match std::fs::symlink_metadata(&dir) {
        Ok(m) if m.is_dir() => {
            if let Some(mode) = fsx::mode_bits(&m)
                && mode & 0o077 != 0
            {
                return Err(StoreError::LoosePermissions { path: dir, mode });
            }
        }
        Ok(_) => {
            return Err(StoreError::Invalid(format!(
                "{} is not a folder (a file or a link); move it aside and open the project again",
                dir.display()
            )));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fsx::ensure_private_dir(&dir)
                .map_err(|e| StoreError::io(format!("cannot create {}", dir.display()), e))?;
        }
        Err(e) => {
            return Err(StoreError::io(format!("cannot read {}", dir.display()), e));
        }
    }
    let ignore = dir.join(".gitignore");
    match fsx::create_private_file(&ignore) {
        Ok(mut f) => f
            .write_all(b"*\n")
            .map_err(|e| StoreError::io(format!("cannot write {}", ignore.display()), e))?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(StoreError::io(
                format!("cannot create {}", ignore.display()),
                e,
            ));
        }
    }
    Ok(dir)
}

fn acquire(dir: &Path) -> Result<ProjectLock, StoreError> {
    let path = dir.join("lock");
    let io_err = |what: &str, e| StoreError::io(format!("cannot {what} {}", path.display()), e);
    // The record is built before the lock is taken and written right after,
    // so a refused opener during a stale-lock takeover can read the previous
    // (dead) holder's record only for the length of that write.
    let holder = LockHolder::current();
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

/// The holder writes its record right after taking the lock; a refusal that
/// lands in between waits briefly for it.
fn read_holder(path: &Path) -> Option<LockHolder> {
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
    fn current_holder_has_a_host() {
        let h = LockHolder::current();
        assert_eq!(h.pid, std::process::id());
        assert!(!h.host.is_empty());
        assert!(!h.host.contains('\n'));
    }
}
