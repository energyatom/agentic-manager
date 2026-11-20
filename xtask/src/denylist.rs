//! `denylist`: department vocabulary appears only under `templates/` and
//! `fixtures/` (AD-23). Scans `crates/` and `web/src` for the seed terms in
//! `xtask/denylist.txt` plus every kebab-case file stem found under
//! `templates/` and `fixtures/` at scan time (seat, document, table, workflow
//! and Source ids are kebab-case). Matching ignores case and needs a token
//! boundary on both sides, so `d1-head` does not match inside `xd1-heads`.

use crate::Failure;
use crate::walk::{files, is_word, rel};
use std::collections::BTreeSet;
use std::path::Path;

const SCANNED: &[&str] = &["crates", "web/src"];
const VOCABULARY_HOMES: &[&str] = &["templates", "fixtures"];

pub fn run(root: &Path, list: &Path) -> Result<String, Failure> {
    let text = std::fs::read_to_string(list)
        .map_err(|e| Failure::Setup(format!("cannot read {}: {e}", list.display())))?;
    let mut terms: BTreeSet<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_ascii_lowercase)
        .collect();
    let seeded = terms.len();
    for home in VOCABULARY_HOMES {
        for path in files(&root.join(home)) {
            if let Some(stem) = id_stem(&path) {
                terms.insert(stem);
            }
        }
    }
    let derived = terms.len() - seeded;

    let mut findings = Vec::new();
    let mut scanned = 0usize;
    for dir in SCANNED {
        for path in files(&root.join(dir)) {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if bytes.iter().take(8192).any(|b| *b == 0) {
                continue; // binary
            }
            scanned += 1;
            let content = String::from_utf8_lossy(&bytes);
            for (n, line) in content.lines().enumerate() {
                let lower = line.to_ascii_lowercase();
                for term in &terms {
                    if contains_token(&lower, term) {
                        findings.push(format!(
                            "{}:{}: `{term}` is department vocabulary; it belongs only under templates/ or fixtures/ (AD-23)",
                            rel(root, &path),
                            n + 1
                        ));
                    }
                }
            }
        }
    }
    if findings.is_empty() {
        Ok(format!(
            "{scanned} files clean against {seeded} seed and {derived} derived terms"
        ))
    } else {
        Err(Failure::Findings(findings))
    }
}

/// A file's stem (name up to its first dot) when it reads as an id: kebab-case
/// with a hyphen. Plain-word stems such as `notes` or `table` are ordinary
/// English, and hidden files are skipped.
fn id_stem(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    if name.starts_with('.') {
        return None;
    }
    let stem = name.split('.').next()?.to_ascii_lowercase();
    let id_like = stem.contains('-')
        && stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && stem.chars().any(|c| c.is_ascii_alphanumeric());
    id_like.then_some(stem)
}

/// `term` occurs in `line` with no word character directly before or after it.
fn contains_token(line: &str, term: &str) -> bool {
    let mut from = 0;
    while let Some(pos) = line[from..].find(term) {
        let start = from + pos;
        let end = start + term.len();
        let before = line[..start].chars().next_back();
        let after = line[end..].chars().next();
        if !before.is_some_and(is_word) && !after.is_some_and(is_word) {
            return true;
        }
        // The match starts with an ASCII byte, so one byte on is a char boundary.
        from = start + 1;
    }
    false
}
