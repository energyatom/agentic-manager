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
fn git_ignored_files_are_not_scanned_but_untracked_ones_are() {
    let tmp = tempfile::tempdir().unwrap();
    let init = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(tmp.path())
        .status()
        .expect("git runs");
    assert!(init.success());
    write(tmp.path(), ".gitignore", "crates/x/private-out.md\n");
    write(tmp.path(), "crates/x/src/lib.rs", "pub fn generic() {}\n");
    write(tmp.path(), "crates/x/private-out.md", "seat d1-head said\n");
    let out = scan(tmp.path());
    assert!(
        out.status.success(),
        "an ignored local file is not scanned: {}",
        stderr(&out)
    );

    write(tmp.path(), "crates/x/notes.md", "seat d1-head said\n");
    let out = scan(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "untracked files are scanned: {err}");
    assert!(
        err.contains("crates/x/notes.md:1"),
        "names file:line: {err}"
    );
    assert!(
        !err.contains("private-out.md"),
        "ignored file stays out: {err}"
    );
}

#[test]
fn current_repo_passes() {
    let out = xtask(&["denylist"]);
    assert!(out.status.success(), "repo is clean: {}", stderr(&out));
}

/// Story 1.5: every id the sample company ships is department vocabulary: a
/// seed term in `xtask/denylist.txt` or a derived kebab-case file stem.
/// Written into a scanned file, each must be reported. The Owner's seat id
/// `owner` is ordinary product vocabulary (`owner_seat`) and is the one
/// recorded exception.
#[test]
fn every_sample_id_is_denied() {
    let sample = repo_root().join("templates/samples/company-a");
    assert!(sample.is_dir(), "the sample ships at {}", sample.display());
    let stems = |dir: &str, ext: &str| -> Vec<String> {
        std::fs::read_dir(sample.join(dir))
            .unwrap_or_else(|e| panic!("{dir}: {e}"))
            .filter_map(|e| {
                let name = e.unwrap().file_name().to_string_lossy().into_owned();
                name.strip_suffix(ext).map(str::to_string)
            })
            .collect()
    };
    let mut ids: Vec<String> = [
        "company-a",
        "d1-inputs",
        "document-a",
        "report-r1",
        "d1-opinion",
        "d1-summary",
        "d1-folder-digest",
        "f1",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    ids.extend(stems("seats", ".md").into_iter().filter(|s| s != "owner"));
    ids.extend(stems("tables", ".yaml"));
    ids.extend(stems("workflows", ".yaml"));
    ids.extend(stems("sources", ".yaml"));
    ids.extend(
        std::fs::read_dir(sample.join("skills"))
            .expect("skills/")
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.')),
    );
    assert!(ids.len() >= 8 + 9 + 1 + 4 + 2 + 2, "{ids:?}");

    // A scan root holding the real templates/ (so derived stems count) and
    // one crates/ file naming every id, one per line.
    let tmp = tempfile::tempdir().unwrap();
    copy_tree(
        &repo_root().join("templates"),
        &tmp.path().join("templates"),
    );
    let body: String = ids.iter().map(|id| format!("\"{id}\"\n")).collect();
    write(tmp.path(), "crates/x/src/lib.rs", &body);
    let out = scan(tmp.path());
    let err = stderr(&out);
    assert!(!out.status.success(), "sample ids must fail: {err}");
    for (n, id) in ids.iter().enumerate() {
        let want = format!("crates/x/src/lib.rs:{}: `{id}`", n + 1);
        assert!(err.contains(&want), "`{id}` is not denied:\n{err}");
    }
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}
