//! Small filesystem helpers shared by the scans.

use std::path::{Path, PathBuf};

/// Folders no scan descends into: build output, packages and hidden folders.
fn skipped(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "target" | "node_modules" | "dist")
}

/// Every file under `dir` (recursively, sorted), skipping build output and
/// hidden folders. A missing `dir` yields nothing.
pub fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(dir, &mut out);
    out.sort();
    out
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
