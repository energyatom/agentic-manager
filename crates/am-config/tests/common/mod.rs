//! Helpers shared by the story 1.12 test files: fixture paths, temporary
//! copies and finding lookups.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use am_config::{ConfigSnapshot, Finding, LoadOptions, Severity, UserAccounts, load, load_in};

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

/// `fixtures/accounts/<variant>.yaml`: the user-level Accounts a fixture
/// project names (story 2.12, AD-17). Tests hand it to the loader by path;
/// they never read the real app data dir.
pub fn accounts_fixture(variant: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/accounts")
        .join(format!("{variant}.yaml"))
}

/// The user list of a valid `fixtures/accounts/<variant>.yaml` (it loads
/// with no finding).
pub fn user_accounts(variant: &str) -> UserAccounts {
    let loaded = am_config::user_accounts::load(&accounts_fixture(variant));
    assert!(
        loaded.findings.is_empty(),
        "{variant}: {:#?}",
        loaded.findings
    );
    loaded.accounts
}

/// The findings of the project at `root` with the user list of `variant`
/// passed, as `agentic validate` passes this Mac's list.
pub fn findings_with(root: &Path, variant: &str) -> Vec<Finding> {
    let opts = LoadOptions {
        user_accounts: Some(user_accounts(variant)),
        ..LoadOptions::default()
    };
    load_in(root, opts)
        .expect("the project folder exists")
        .findings
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
