//! Small filesystem helpers shared by the scans.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Folders no scan descends into: build output, packages and hidden folders.
fn skipped(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "target" | "node_modules" | "dist")
}

/// Every file under `dir` (recursively, sorted), skipping build output and
/// hidden folders. Inside a git work tree it lists tracked and untracked files
/// but not git-ignored ones (private local outputs never reach a scan); outside
/// one it walks the folder. A missing `dir` yields nothing.
pub fn files(dir: &Path) -> Vec<PathBuf> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut out = git_files(dir).unwrap_or_else(|| {
        let mut walked = Vec::new();
        collect(dir, &mut walked);
        walked
    });
    out.sort();
    out
}

/// `git ls-files --cached --others --exclude-standard` under `dir`, or `None`
/// when `dir` is not in a git work tree (or git is unavailable).
fn git_files(dir: &Path) -> Option<Vec<PathBuf>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let listed = String::from_utf8_lossy(&out.stdout).into_owned();
    Some(
        listed
            .split('\0')
            .filter(|p| !p.is_empty())
            .filter(|p| {
                let parts: Vec<&str> = p.split('/').collect();
                !parts[..parts.len() - 1].iter().any(|d| skipped(d))
            })
            .map(|p| dir.join(p))
            .filter(|p| p.is_file())
            .collect(),
    )
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !skipped(&name) {
                collect(&path, out);
            }
        } else if kind.is_file() {
            out.push(path);
        }
    }
}

/// `path` relative to `root`, with forward slashes.
pub fn rel(root: &Path, path: &Path) -> String {
    let p = path.strip_prefix(root).unwrap_or(path);
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// True for ASCII letters, digits and `_`: the characters that continue a token.
pub fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
