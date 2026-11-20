//! `cargo xtask denylist`: department vocabulary stays under templates/ and
//! fixtures/ (AD-23; story 1.2, written first). The sample terms below live in
//! xtask/, which the scan does not cover.
#![forbid(unsafe_code)]

mod common;

use common::{repo_root, stderr, write, xtask};

fn list() -> String {
    repo_root()
        .join("xtask/denylist.txt")
        .to_string_lossy()
        .into_owned()
}

fn scan(root: &std::path::Path) -> std::process::Output {
    xtask(&[
        "denylist",
        "--root",
        &root.to_string_lossy(),
        "--list",
        &list(),
    ])
}

#[test]
fn sample_id_in_crates_fails_with_file_line_and_term() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/x/src/lib.rs",
        "//! generic\n\npub const SEAT: &str = \"d1-head\";\n",
    );
    let out = scan(tmp.path());
    let err = stderr(&out);
    assert!(
        !out.status.success(),
        "a sample id in crates/ must fail: {err}"
    );
    assert!(
        err.contains("crates/x/src/lib.rs:3"),
        "names file:line: {err}"
    );
    assert!(err.contains("d1-head"), "names the term: {err}");
}

#[test]
fn sample_id_in_web_src_fails() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "web/src/App.tsx",
        "export const t = 'Table-T1';\n",
    );
    let out = scan(tmp.path());
    let err = stderr(&out);
    assert!(
        !out.status.success(),
        "a sample id in web/src must fail: {err}"
    );
    assert!(err.contains("web/src/App.tsx:1"), "names file:line: {err}");
    assert!(err.contains("table-t1"), "names the term: {err}");
}

#[test]
fn sample_id_in_fixtures_passes() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "fixtures/sample/seat.md", "id: d1-head\n");
    write(tmp.path(), "crates/x/src/lib.rs", "pub fn generic() {}\n");
    let out = scan(tmp.path());
    assert!(
        out.status.success(),
        "fixtures may hold it: {}",
        stderr(&out)
    );
}

#[test]
fn file_stem_under_templates_is_denied_in_crates() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "templates/samples/zeta/seats/z9-lead.md",
        "name: Z9 Lead\n",
    );
    write(
        tmp.path(),
        "crates/x/src/lib.rs",
        "pub const S: &str = \"z9-lead\";\n",
    );
    let out = scan(tmp.path());
    let err = stderr(&out);
    assert!(
        !out.status.success(),
        "a template file stem must be denied: {err}"
    );
    assert!(
        err.contains("crates/x/src/lib.rs:1"),
        "names file:line: {err}"
    );
    assert!(err.contains("z9-lead"), "names the term: {err}");
}

#[test]
fn a_term_inside_a_longer_id_is_not_a_match() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "crates/x/src/lib.rs",
        "pub const S: &str = \"xd1-heads\";\n",
    );
    let out = scan(tmp.path());
    assert!(
        out.status.success(),
        "no partial-token match: {}",
        stderr(&out)
    );
}

#[test]
fn current_repo_passes() {
    let out = xtask(&["denylist"]);
    assert!(out.status.success(), "repo is clean: {}", stderr(&out));
}
