//! `cargo xtask check-forbid`: every crate forbids `unsafe` (story 1.2,
//! written first).
#![forbid(unsafe_code)]

mod common;

use common::{stderr, write, xtask};

const MANIFEST: &str = "[package]\nname = \"a\"\n\n[lints]\nworkspace = true\n";
const ROOT_OK: &str = "//! ok\n#![forbid(unsafe_code)]\n\npub fn f() {}\n";

fn check(root: &std::path::Path) -> std::process::Output {
    xtask(&["check-forbid", "--root", &root.to_string_lossy()])
}

#[test]
fn compliant_tree_passes() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "crates/a/Cargo.toml", MANIFEST);
    write(tmp.path(), "crates/a/src/lib.rs", ROOT_OK);
    write(
        tmp.path(),
        "crates/a/src/words.rs",
        "// unsafe { } in a comment\npub const W: &str = \"unsafe { }\";\npub const R: &str = r#\"unsafe fn\"#;\n",
    );
    let out = check(tmp.path());
    assert!(out.status.success(), "compliant: {}", stderr(&out));
}

#[test]
fn crate_root_without_the_attribute_fails_naming_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "crates/a/Cargo.toml", MANIFEST);
    write(tmp.path(), "crates/a/src/lib.rs", ROOT_OK);
    write(tmp.path(), "crates/b/Cargo.toml", MANIFEST);
    write(tmp.path(), "crates/b/src/main.rs", "fn main() {}\n");
    let out = check(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "missing attribute must fail: {err}");
    assert!(
        err.contains("crates/b/src/main.rs"),
        "names the file: {err}"
    );
}

#[test]
fn unsafe_block_fails_naming_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "crates/a/Cargo.toml", MANIFEST);
    write(tmp.path(), "crates/a/src/lib.rs", ROOT_OK);
    write(
        tmp.path(),
        "crates/a/src/raw.rs",
        "pub fn f(p: *const u8) -> u8 {\n    unsafe { *p }\n}\n",
    );
    let out = check(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "unsafe code must fail: {err}");
    assert!(
        err.contains("crates/a/src/raw.rs:2"),
        "names file:line: {err}"
    );
}

#[test]
fn manifest_without_workspace_lints_fails() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/a/Cargo.toml",
        "[package]\nname = \"a\"\n",
    );
    write(tmp.path(), "crates/a/src/lib.rs", ROOT_OK);
    let out = check(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "missing [lints] must fail: {err}");
    assert!(
        err.contains("crates/a/Cargo.toml"),
        "names the manifest: {err}"
    );
}

#[test]
fn current_repo_passes() {
    let out = xtask(&["check-forbid"]);
    assert!(
        out.status.success(),
        "repo forbids unsafe: {}",
        stderr(&out)
    );
}
