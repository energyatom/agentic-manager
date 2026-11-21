//! The licence policy refuses a GPL crate (AD-3; story 1.2, written first).
//! Runs cargo-deny offline on `fixtures/supply-chain/gpl-consumer`, which
//! depends on the GPL-3.0-only `gpl-probe` fixture crate. Ignored in the
//! default run so `just check` needs no cargo-deny; `just supply-chain` runs it.
#![forbid(unsafe_code)]

mod common;

use common::{repo_root, stderr, stdout};
use std::process::Command;

#[test]
#[ignore = "needs cargo-deny; run by just supply-chain"]
fn gpl_dependency_fails_the_licence_policy_naming_crate_and_licence() {
    let root = repo_root();
    let out = Command::new("cargo")
        .args(["deny", "--manifest-path"])
        .arg(root.join("fixtures/supply-chain/gpl-consumer/Cargo.toml"))
        .arg("--config")
        .arg(root.join("deny.toml"))
        .args(["--frozen", "--color", "never", "check", "licenses"])
        .current_dir(&root)
        .output()
        .expect("cargo-deny is installed (brew install cargo-deny)");
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        !text.contains("no such command"),
        "cargo-deny must be installed: {text}"
    );
    assert!(!out.status.success(), "GPL must fail the policy: {text}");
    assert!(text.contains("gpl-probe"), "names the crate: {text}");
    assert!(text.contains("GPL-3.0-only"), "names the licence: {text}");
    assert!(text.contains("rejected"), "states the rejection: {text}");
    assert!(
        !text.contains("falling back to default config"),
        "runs the repo policy, not cargo-deny's default: {text}"
    );
    assert_eq!(
        text.matches("error[rejected]").count(),
        1,
        "only the GPL crate is rejected; the MIT consumer is allowed: {text}"
    );
}

/// A CC0-1.0 crate other than notify still fails the licence policy (story
/// 1.9: the exception names notify@8.2.2 only). Runs cargo-deny offline on
/// `fixtures/supply-chain/cc0-consumer`, which depends on the CC0-1.0
/// `cc0-probe` fixture crate.
#[test]
#[ignore = "needs cargo-deny; run by just supply-chain"]
fn cc0_crate_other_than_notify_fails_the_licence_policy() {
    let root = repo_root();
    let out = Command::new("cargo")
        .args(["deny", "--manifest-path"])
        .arg(root.join("fixtures/supply-chain/cc0-consumer/Cargo.toml"))
        .arg("--config")
        .arg(root.join("deny.toml"))
        .args(["--frozen", "--color", "never", "check", "licenses"])
        .current_dir(&root)
        .output()
        .expect("cargo-deny is installed (brew install cargo-deny)");
    let text = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        !text.contains("no such command"),
        "cargo-deny must be installed: {text}"
    );
    assert!(
        !out.status.success(),
        "a second CC0 crate must fail the policy: {text}"
    );
    assert!(text.contains("cc0-probe"), "names the crate: {text}");
    assert!(text.contains("CC0-1.0"), "names the licence: {text}");
    assert!(text.contains("rejected"), "states the rejection: {text}");
    assert!(
        !text.contains("falling back to default config"),
        "runs the repo policy, not cargo-deny's default: {text}"
    );
    assert_eq!(
        text.matches("error[rejected]").count(),
        1,
        "only the CC0 probe is rejected; the MIT consumer is allowed: {text}"
    );
}

/// The allow-list is exactly AD-3's nine licences (MIT-0 added by the
/// architect REPLAN, 2026-11-20), and the one exception is CC0-1.0 for
/// `notify@8.2.2` only (AD-3, Sofia REPLAN 2026-11-21; story 1.9).
#[test]
fn deny_toml_allows_the_nine_licences_and_only_the_notify_cc0_exception() {
    let text = std::fs::read_to_string(repo_root().join("deny.toml")).expect("deny.toml exists");
    let start = text.find("allow = [").expect("[licenses] allow list");
    let end = start + text[start..].find(']').expect("closed list");
    let mut allowed: Vec<String> = text[start + "allow = [".len()..end]
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect();
    allowed.sort();
    let mut expected = vec![
        "MIT",
        "MIT-0",
        "Apache-2.0",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "ISC",
        "Zlib",
        "Unicode-3.0",
        "MPL-2.0",
    ];
    expected.sort();
    assert_eq!(allowed, expected, "CC0-1.0 is never on the allow-list");

    // No inline `exceptions = [...]`: the one exception is a table.
    assert!(
        !text
            .lines()
            .any(|l| l.trim_start().starts_with("exceptions")),
        "exceptions only as [[licenses.exceptions]] tables"
    );
    let tables: Vec<&str> = text.split("[[licenses.exceptions]]").skip(1).collect();
    assert_eq!(tables.len(), 1, "exactly one licence exception: {text}");
    // The table's own lines, up to the next table header.
    let body: Vec<String> = tables[0]
        .lines()
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with('['))
        .map(|l| l.split('#').next().unwrap_or("").trim().replace(' ', ""))
        .filter(|l| !l.is_empty())
        .collect();
    let mut body_sorted = body.clone();
    body_sorted.sort();
    assert!(
        body_sorted == ["allow=[\"CC0-1.0\"]", "crate=\"notify@8.2.2\""]
            || body_sorted == ["allow=[\"CC0-1.0\"]", "crate=\"notify@=8.2.2\""],
        "the exception is CC0-1.0 for notify@8.2.2 only: {body:?}"
    );
}

/// notify is pinned exactly at 8.2.2 with default features, linked into
/// am-config, and its exception is recorded in THIRD_PARTY.md and its crate
/// note (story 1.9).
#[test]
fn notify_licence_exception_is_pinned_at_8_2_2_and_recorded() {
    let root = repo_root();
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let pins: Vec<&str> = manifest
        .lines()
        .filter(|l| l.trim_start().starts_with("notify"))
        .collect();
    assert_eq!(
        pins,
        ["notify = \"=8.2.2\""],
        "one exact pin with default features in [workspace.dependencies]"
    );
    let config = std::fs::read_to_string(root.join("crates/am-config/Cargo.toml")).unwrap();
    assert!(
        config
            .lines()
            .any(|l| l.trim().replace(' ', "") == "notify.workspace=true"),
        "am-config takes notify from the workspace"
    );
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
    assert!(
        lock.contains("name = \"notify\"\nversion = \"8.2.2\"\n"),
        "Cargo.lock links notify 8.2.2"
    );
    let third = std::fs::read_to_string(root.join("THIRD_PARTY.md")).unwrap();
    assert!(
        third.contains("notify") && third.contains("CC0-1.0"),
        "THIRD_PARTY.md names the exception"
    );
    let note = std::fs::read_to_string(root.join("docs/crate-notes/notify.md"))
        .expect("docs/crate-notes/notify.md exists");
    assert!(note.contains("8.2.2"), "the note names the pinned version");
    assert!(note.contains("CC0-1.0"), "the note names the exception");
}
