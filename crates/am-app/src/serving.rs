//! What `agentic serve` does around the server (story 3.2, AD-13): the
//! owner-only check at start, and the per-launch CLI token's file.
//!
//! - [`owner_only_at_start`]: before it binds, the server refuses to start,
//!   naming the path and the `chmod` that fixes it, when the app data dir
//!   or the open project's runtime folder, or any folder inside either at
//!   any depth, is not owner-only (0700), or a file in one of them can be
//!   read by others (not 0600); a symbolic link anywhere inside refuses it
//!   too, and so does a folder that cannot be looked at (only a missing one
//!   passes). It is 2.12's check ([`crate::user::owner_only`]) extended to
//!   the whole tree and to the runtime folder (review P1).
//! - [`write_cli_token`]: the token's 64 hex digits in
//!   `AppDataLayout::cli_token(pid)` = `<app data dir>/serving/<pid>.token`,
//!   0600 in the 0700 `serving` folder (both created owner-only), beside
//!   the port record; [`clear_cli_token`] removes it at exit. A stale file
//!   from a dead process is harmless: the server compares against the value
//!   it holds in memory.

use std::path::{Path, PathBuf};

use am_domain::RuntimeLayout;
use am_server::CliToken;

use crate::runtime::{Places, Refusal, ensure_private_dir, replace_file};

/// Refuses the start when a folder the server keeps state in, or anything
/// inside it at any depth, is not owner-only.
pub fn owner_only_at_start(
    places: &Places,
    runtime: Option<&RuntimeLayout>,
) -> Result<(), Refusal> {
    crate::user::owner_only(&places.user).map_err(Refusal)?;
    owner_only_tree(&places.app_data, "this Mac's app data folder")?;
    if let Some(rt) = runtime {
        owner_only_tree(rt.dir(), "the project's runtime folder")?;
    }
    Ok(())
}

/// The words of a start refused for `path`.
#[cfg(unix)]
fn refused(words: String) -> Refusal {
    Refusal(format!(
        "agentic serve did not start: {words}, then run agentic serve again"
    ))
}

/// The tree at `top`: every folder 0700 and every other entry 0600, at any
/// depth, and no symbolic link anywhere (the top included). A missing `top`
/// passes (it is created owner-only); any other error looking at it
/// refuses. An entry that vanishes during the walk (a temporary file
/// renamed meanwhile) is skipped.
#[cfg(unix)]
fn owner_only_tree(top: &Path, what: &str) -> Result<(), Refusal> {
    match std::fs::symlink_metadata(top) {
        Ok(m) => owner_only_entry(top, &m, what),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(refused(format!(
            "{what} {} cannot be checked ({e}); make sure you own it and it is reachable, run `chmod 700 {}`",
            top.display(),
            top.display()
        ))),
    }
}

#[cfg(unix)]
fn owner_only_entry(path: &Path, m: &std::fs::Metadata, what: &str) -> Result<(), Refusal> {
    use std::os::unix::fs::PermissionsExt;
    let mode = m.permissions().mode() & 0o777;
    let ft = m.file_type();
    if ft.is_symlink() {
        return Err(refused(format!(
            "{} is a symbolic link inside {what}, which must hold only its own files and folders; remove it (`rm {}`)",
            path.display(),
            path.display()
        )));
    }
    if !ft.is_dir() {
        if mode & 0o077 != 0 {
            return Err(refused(format!(
                "{} can be read by other users (mode {mode:o}); run `chmod 600 {}`",
                path.display(),
                path.display()
            )));
        }
        return Ok(());
    }
    if mode & 0o077 != 0 {
        return Err(refused(format!(
            "{} in {what} can be read or entered by other users (mode {mode:o}); run `chmod 700 {}`",
            path.display(),
            path.display()
        )));
    }
    let cannot_read = |e: std::io::Error| {
        refused(format!(
            "{} in {what} cannot be read ({e}); run `chmod 700 {}`",
            path.display(),
            path.display()
        ))
    };
    for entry in std::fs::read_dir(path).map_err(cannot_read)? {
        let entry = entry.map_err(cannot_read)?;
        let child = entry.path();
        match std::fs::symlink_metadata(&child) {
            Ok(m) => owner_only_entry(&child, &m, what)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(refused(format!(
                    "{} in {what} cannot be checked ({e}); run `chmod 600 {}` (a file) or `chmod 700 {}` (a folder)",
                    child.display(),
                    child.display(),
                    child.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn owner_only_tree(_top: &Path, _what: &str) -> Result<(), Refusal> {
    Ok(())
}

/// Writes this launch's CLI token, owner-only, to
/// `<app data dir>/serving/<pid>.token` (the folders created 0700 when
/// missing); returns the file's path.
pub fn write_cli_token(places: &Places, pid: u32, token: &CliToken) -> Result<PathBuf, Refusal> {
    ensure_private_dir(&places.app_data)?;
    ensure_private_dir(&places.user.serving_dir())?;
    let path = places.user.cli_token(pid);
    replace_file(&path, format!("{}\n", token.expose()).as_bytes())?;
    Ok(path)
}

/// Removes the CLI token file of process `pid` (at `serve`'s exit).
pub fn clear_cli_token(places: &Places, pid: u32) {
    let _ = std::fs::remove_file(places.user.cli_token(pid));
}

/// Whether `agentic serve`'s stdin is a terminal, so pressing Enter there
/// can print a new pairing code (review P9). With stdin redirected (a
/// service, `< /dev/null`, a test) nothing reads it.
pub fn stdin_reads_the_owner() -> bool {
    use std::io::IsTerminal as _;
    std::io::stdin().is_terminal()
}
