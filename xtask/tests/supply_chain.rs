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

/// The allow-list is exactly AD-3's eight licences, with no exceptions.
#[test]
fn deny_toml_allows_exactly_the_eight_licences_and_no_exceptions() {
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
        "Apache-2.0",
        "BSD-2-Clause",
        "BSD-3-Clause",
        "ISC",
        "Zlib",
        "Unicode-3.0",
        "MPL-2.0",
    ];
    expected.sort();
    assert_eq!(allowed, expected);
    assert!(
        !text
            .lines()
            .any(|l| l.trim_start().starts_with("exceptions")),
        "no licence exceptions (story 1.2 Never)"
    );
}
