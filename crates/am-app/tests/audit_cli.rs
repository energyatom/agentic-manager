//! Story 1.7: `agentic audit export <project> [--out <file>]` and
//! `agentic audit verify <file>` as the owner runs them. Each test runs the
//! real `agentic` binary (`CARGO_BIN_EXE_agentic`) and checks its output and
//! exit code: export prints the entry count and head hash; verify exits 0 on
//! OK, 1 naming the first broken entry, 2 on bad usage.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Output};

use am_domain::Actor;
use am_store::{SCHEMA_VERSION, Store, WriteCommand, open_project};

fn agentic(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agentic"))
        .args(args)
        .output()
        .unwrap()
}

fn text(o: &Output) -> (i32, String, String) {
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn sys() -> Actor {
    Actor::System {
        component: "cli-test".into(),
    }
}

/// A project with a run and an audit entry; the store stays open (and the
/// project locked) while it lives.
fn populated(root: &Path) -> Store {
    let (store, _) = open_project(root).unwrap();
    let w = store.writer();
    w.exec(
        sys(),
        WriteCommand::CreateRun {
            run_id: "run-1".into(),
            workflow: "wf-1".into(),
            origin: "manual".into(),
            steps: vec!["a".into()],
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::FinishRun {
            run_id: "run-1".into(),
            status: "succeeded".into(),
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::Audit {
            action: "egress.blocked".into(),
            target: "host-1".into(),
            outcome: "not on the allow-list".into(),
            hash: None,
        },
    )
    .unwrap();
    store
}

fn p(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// The head hash printed after "head hash ".
fn head_in(s: &str) -> String {
    let i = s.find("head hash ").expect(s) + "head hash ".len();
    s[i..].chars().take_while(char::is_ascii_hexdigit).collect()
}

#[test]
fn export_to_a_file_prints_count_and_head_and_verify_says_ok() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    // The store stays open: export reads while the project is locked.
    let store = populated(&project);
    let out = dir.path().join("audit.jsonl");

    let (code, stdout, stderr) = text(&agentic(&[
        "audit",
        "export",
        p(&project),
        "--out",
        p(&out),
    ]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let file = std::fs::read_to_string(&out).unwrap();
    let count = file.lines().count();
    let expected = SCHEMA_VERSION as usize + 3;
    assert_eq!(count, expected, "{file}");
    assert!(stdout.contains(&format!("{count} entries")), "{stdout}");
    let head = head_in(&stdout);
    assert_eq!(head.len(), 64, "{stdout}");
    assert!(file.lines().last().unwrap().contains(&head));
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&out).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the export is owner-only");
    }

    let (code, stdout, stderr) = text(&agentic(&["audit", "verify", p(&out)]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("OK"), "{stdout}");
    assert!(stdout.contains(&format!("{count} entries")), "{stdout}");
    assert_eq!(head_in(&stdout), head);
    drop(store);
}

#[test]
fn export_without_out_writes_json_lines_to_stdout() {
    let dir = tempfile::tempdir().unwrap();
    drop(populated(dir.path()));
    let (code, stdout, stderr) = text(&agentic(&["audit", "export", p(dir.path())]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.lines().all(|l| l.starts_with("{\"")), "{stdout}");
    assert!(stdout.contains("\"egress.blocked\""), "{stdout}");
    let count = stdout.lines().count();
    assert!(stderr.contains(&format!("{count} entries")), "{stderr}");
    assert_eq!(head_in(&stderr).len(), 64, "{stderr}");
}

#[test]
fn verify_names_the_altered_entry_and_exits_one() {
    let dir = tempfile::tempdir().unwrap();
    drop(populated(dir.path()));
    let out = dir.path().join("audit.jsonl");
    let (code, ..) = text(&agentic(&[
        "audit",
        "export",
        p(dir.path()),
        "--out",
        p(&out),
    ]));
    assert_eq!(code, 0);
    let good = std::fs::read_to_string(&out).unwrap();
    let line = good
        .lines()
        .position(|l| l.contains("\"target\":\"host-1\""))
        .unwrap()
        + 1;
    let seq = SCHEMA_VERSION as usize + 3;
    std::fs::write(
        &out,
        good.replace("\"target\":\"host-1\"", "\"target\":\"host-2\""),
    )
    .unwrap();

    let (code, stdout, stderr) = text(&agentic(&["audit", "verify", p(&out)]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    let all = format!("{stdout}{stderr}");
    assert!(all.contains(&format!("line {line}")), "{all}");
    assert!(all.contains(&format!("entry {seq}")), "{all}");
}

#[test]
fn verify_names_a_line_that_is_not_json() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("audit.jsonl");
    std::fs::write(&file, "garbage\n").unwrap();
    let (code, stdout, stderr) = text(&agentic(&["audit", "verify", p(&file)]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        format!("{stdout}{stderr}").contains("line 1"),
        "{stdout}{stderr}"
    );
}

#[test]
fn export_refuses_a_database_newer_than_the_binary() {
    let dir = tempfile::tempdir().unwrap();
    let ag = dir.path().join(".agentic");
    std::fs::create_dir(&ag).unwrap();
    am_store::testing::database_at_version(&ag.join("state.db"), SCHEMA_VERSION + 1).unwrap();
    let (code, stdout, stderr) = text(&agentic(&["audit", "export", p(dir.path())]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("newer than this app knows"), "{stderr}");
}

#[test]
fn a_refused_export_leaves_an_existing_out_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    let ag = project.join(".agentic");
    std::fs::create_dir_all(&ag).unwrap();
    am_store::testing::database_at_version(&ag.join("state.db"), SCHEMA_VERSION + 1).unwrap();
    let out = dir.path().join("audit.jsonl");
    std::fs::write(&out, "an earlier export\n").unwrap();
    let (code, stdout, stderr) = text(&agentic(&[
        "audit",
        "export",
        p(&project),
        "--out",
        p(&out),
    ]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("newer than this app knows"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        "an earlier export\n"
    );
    let left: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left.len(), 2, "no temporary file left: {left:?}");
}

#[test]
fn out_inside_the_project_agentic_folder_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    drop(populated(dir.path()));
    let db = dir.path().join(".agentic").join("state.db");
    let before = std::fs::read(&db).unwrap();
    for target in [db.clone(), dir.path().join(".agentic").join("audit.jsonl")] {
        let (code, stdout, stderr) = text(&agentic(&[
            "audit",
            "export",
            p(dir.path()),
            "--out",
            p(&target),
        ]));
        assert_eq!(code, 2, "{target:?}: {stdout}{stderr}");
        assert!(stderr.contains(".agentic"), "{stderr}");
    }
    assert_eq!(std::fs::read(&db).unwrap(), before, "state.db untouched");
    assert!(!dir.path().join(".agentic").join("audit.jsonl").exists());
}

#[test]
fn export_of_a_folder_without_a_project_database_fails() {
    let dir = tempfile::tempdir().unwrap();
    let (code, stdout, stderr) = text(&agentic(&["audit", "export", p(dir.path())]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("state.db"), "{stderr}");
    assert!(!dir.path().join(".agentic").exists(), "nothing is created");
}

#[test]
fn bad_usage_exits_two() {
    for args in [
        vec!["audit"],
        vec!["audit", "frobnicate"],
        vec!["audit", "export"],
        vec!["audit", "export", "/tmp", "--out"],
        vec!["audit", "verify"],
        vec!["audit", "verify", "a.jsonl", "b.jsonl"],
    ] {
        let (code, stdout, stderr) = text(&agentic(&args));
        assert_eq!(code, 2, "{args:?}: {stdout}{stderr}");
        assert!(
            stderr.contains("usage: agentic audit"),
            "{args:?}: {stderr}"
        );
    }
}
