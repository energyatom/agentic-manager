//! Helpers shared by the story 1.12 test files: fixture paths, temporary
//! copies and finding lookups.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use am_config::{ConfigSnapshot, Finding, Severity, load};

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/projects")
        .join(name)
}

pub fn copy_dir(from: &Path, to: &Path) {
    // A stream copy (story 1.13: never `std::fs::copy`).
    am_testkit::copy_tree(from, to);
}

/// A copy of a fixture project in a temporary folder, to change files in.
pub fn copy_of(name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture(name), dir.path());
    dir
}

pub fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

pub fn edit(root: &Path, rel: &str, from: &str, to: &str) {
    let text = std::fs::read_to_string(root.join(rel)).unwrap();
    assert!(text.contains(from), "{rel} has no `{from}`");
    write(root, rel, &text.replacen(from, to, 1));
}

pub fn findings(root: &Path) -> Vec<Finding> {
    load(root).expect("the project folder exists").findings
}

pub fn snapshot(root: &Path) -> ConfigSnapshot {
    let loaded = load(root).expect("the project folder exists");
    let errors: Vec<&Finding> = loaded
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    loaded.snapshot.expect("no errors gives a snapshot")
}

pub fn errors(found: &[Finding]) -> Vec<&Finding> {
    found
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .collect()
}

pub fn warnings(found: &[Finding]) -> Vec<&Finding> {
    found
        .iter()
        .filter(|f| f.severity == Severity::Warning)
        .collect()
}

/// The findings of one file.
pub fn in_file<'a>(found: &'a [Finding], file: &str) -> Vec<&'a Finding> {
    found.iter().filter(|f| f.file == file).collect()
}

/// The one finding at `file:line` whose message contains `message`.
pub fn at<'a>(found: &'a [Finding], file: &str, line: u32, message: &str) -> &'a Finding {
    found
        .iter()
        .find(|f| f.file == file && f.line == line && f.message.contains(message))
        .unwrap_or_else(|| panic!("no finding {file}:{line}: {message} in {found:#?}"))
}

/// A sorted list of tool ids, for comparing grants.
pub fn set(ids: &[&str]) -> std::collections::BTreeSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// `company.yaml` of the small projects the story 1.14 tests write, at this
/// app's `schema_version` (2).
pub const COMPANY_V2: &str = "schema_version: 2\nname: Small Project\ntimezone: UTC\ncurrency: EUR\nteams:\n  - id: core\n    name: Core team\n";

/// A project in a temporary folder holding `company.yaml` ([`COMPANY_V2`])
/// and each `(path, text)` of `files`.
pub fn project_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "company.yaml", COMPANY_V2);
    for (rel, text) in files {
        write(dir.path(), rel, text);
    }
    dir
}

/// The patterns `seat` may reach, as text (`ConfigSnapshot::allowed_hosts`,
/// story 1.14: the seat's own `allowed_hosts`).
pub fn hosts_of(snap: &ConfigSnapshot, seat: &str) -> Vec<String> {
    snap.allowed_hosts(seat)
        .iter()
        .map(|h| h.to_string())
        .collect()
}
