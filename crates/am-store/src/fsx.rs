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
