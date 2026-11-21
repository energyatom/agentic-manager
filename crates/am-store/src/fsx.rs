//! Owner-only files and folders (AD-32): folders 0700, files 0600. Unix
//! modes on macOS and Linux; elsewhere the plain call (a project open is
//! refused there before any of this runs).

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

/// Creates `dir` with mode 0700 when it is missing (its parent must exist).
pub(crate) fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        std::fs::DirBuilder::new().mode(0o700).create(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(dir)
    }
}

fn private_options() -> OpenOptions {
    let mut o = OpenOptions::new();
    o.read(true).write(true);
    #[cfg(unix)]
    o.mode(0o600);
    o
}

/// A new, empty file with mode 0600; fails if it exists.
pub(crate) fn create_private_file(path: &Path) -> io::Result<File> {
    let f = private_options().create_new(true).open(path)?;
    #[cfg(unix)]
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(f)
}

/// Creates an empty 0600 file unless one is already there.
pub(crate) fn create_private_file_if_missing(path: &Path) -> io::Result<()> {
    match create_private_file(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

/// Opens (creating 0600 if missing) without truncating.
pub(crate) fn open_private_file(path: &Path) -> io::Result<File> {
    let existed = path.exists();
    let f = private_options().create(true).truncate(false).open(path)?;
    #[cfg(unix)]
    if !existed {
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = existed;
    Ok(f)
}

/// The permission bits of a folder (`None` off Unix).
pub(crate) fn mode_bits(meta: &std::fs::Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        Some(meta.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        None
    }
}

/// Opens `path` for writing as an owner-only (0600) file, truncating it.
pub(crate) fn rewrite_private_file(path: &Path) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    o.mode(0o600);
    let f = o.open(path)?;
    #[cfg(unix)]
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(f)
}

/// Sets a folder to 0700 or a file to 0600 (no-op off Unix).
pub(crate) fn make_private(path: &Path, dir: bool) -> io::Result<()> {
    #[cfg(unix)]
    {
        let mode = if dir { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, dir);
        Ok(())
    }
}

/// The lowercase sha256 hex of a file's bytes, read as a stream.
pub(crate) fn sha256_file(path: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Copies `src` to `dst` (owner-only, replaced if present) by reading and
/// writing its bytes, and returns their sha256 hex. Never `std::fs::copy`,
/// whose first step on macOS is `clonefile(2)`: that call fails with EPERM
/// on a file under `~/Library/CloudStorage/` that carries no
/// `com.apple.macl` grant for this app, while plain reads succeed (story 1.13).
pub fn stream_copy(src: &Path, dst: &Path) -> io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    #[cfg(feature = "test-support")]
    testing::note_stream_copy(src);
    let mut from = File::open(src)?;
    let mut to = rewrite_private_file(dst)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        to.write_all(&buf[..n])?;
    }
    to.sync_all()?;
    Ok(hex::encode(h.finalize()))
}

/// Test support: a file armed as one whose clone is refused (EPERM), as
/// under `~/Library/CloudStorage/` without a `com.apple.macl` grant. The
/// store has no clone path to count (clippy bans `std::fs::copy`, whose
/// first step is `clonefile(2)`); what is counted is that an armed file
/// went through [`stream_copy`].
#[cfg(feature = "test-support")]
pub(crate) mod testing {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static REFUSED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    static STREAM_COPIES: AtomicUsize = AtomicUsize::new(0);

    pub fn refuse_clone_of(path: &Path) {
        if let Ok(mut r) = REFUSED.lock() {
            r.push(path.to_path_buf());
        }
    }

    fn refused(path: &Path) -> bool {
        REFUSED.lock().is_ok_and(|r| r.iter().any(|p| p == path))
    }

    pub(crate) fn note_stream_copy(src: &Path) {
        if refused(src) {
            STREAM_COPIES.fetch_add(1, Ordering::SeqCst);
        }
    }

    pub fn stream_copies_of_refused() -> usize {
        STREAM_COPIES.load(Ordering::SeqCst)
    }
}
