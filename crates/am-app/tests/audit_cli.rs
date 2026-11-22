//! Story 1.7: `agentic audit export <project> [--out <file>]` and
//! `agentic audit verify <file>` as the owner runs them. Each test runs the
//! real `agentic` binary (`CARGO_BIN_EXE_agentic`) and checks its output and
//! exit code: export prints the entry count and head hash; verify exits 0 on
//! OK, 1 naming the first broken entry, 2 on bad usage. Since story 1.13 the
//! project's database is in its runtime folder outside the project (found
//! read-only from `project_id`), so each test runs with HOME (and
//! XDG_DATA_HOME) in its temporary folder.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Output};

use am_domain::runtime::{RUNTIME_BASE_NAME, legacy_dir};
use am_domain::{Actor, ProjectId, RuntimeBase, RuntimeLayout};
use am_store::{SCHEMA_VERSION, Store, WriteCommand};

/// The runtime base `agentic` resolves with HOME and XDG_DATA_HOME in `dir`.
fn base(dir: &Path) -> RuntimeBase {
    let support = if cfg!(target_os = "macos") {
        dir.join(".home/Library/Application Support")
    } else {
        dir.join(".xdg")
    };
    RuntimeBase::new(support.join(RUNTIME_BASE_NAME))
}

fn agentic(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agentic"))
        .args(args)
        .env("HOME", dir.join(".home"))
        .env("XDG_DATA_HOME", dir.join(".xdg"))
        .output()
        .unwrap()
}

/// A project at `root` with an id and its (owner-only) runtime folder under
/// the base for `dir`.
fn project(dir: &Path, root: &Path) -> RuntimeLayout {
    std::fs::create_dir_all(root).unwrap();
    let id = ProjectId::mint();
    std::fs::write(
        root.join("company.yaml"),
        format!("schema_version: 1\nproject_id: {id}\nname: X\ntimezone: UTC\ncurrency: EUR\n"),
    )
    .unwrap();
    let layout = base(dir).project(&id);
    std::fs::create_dir_all(layout.dir()).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(layout.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
    layout
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
fn populated(dir: &Path, root: &Path) -> Store {
    let layout = project(dir, root);
    let (store, _) = am_store::open_runtime(&layout, "test-mac".into()).unwrap();
    let w = store.writer();
    w.exec(
        sys(),
        WriteCommand::CreateRun {
            run_id: "run-1".into(),
            workflow: "wf-1".into(),
            origin: "manual".into(),
            steps: vec!["a".into()],
            // Story 3.1: a run made with no config snapshot has no hash.
            config_hash: None,
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
            args: Default::default(),
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
    let store = populated(dir.path(), &project);
    let out = dir.path().join("audit.jsonl");

    let (code, stdout, stderr) = text(&agentic(
        dir.path(),
        &["audit", "export", p(&project), "--out", p(&out)],
    ));
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

    let (code, stdout, stderr) = text(&agentic(dir.path(), &["audit", "verify", p(&out)]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("OK"), "{stdout}");
    assert!(stdout.contains(&format!("{count} entries")), "{stdout}");
    assert_eq!(head_in(&stdout), head);
    drop(store);
}

#[test]
fn export_without_out_writes_json_lines_to_stdout() {
    let dir = tempfile::tempdir().unwrap();
    drop(populated(dir.path(), dir.path()));
    let (code, stdout, stderr) = text(&agentic(dir.path(), &["audit", "export", p(dir.path())]));
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
    drop(populated(dir.path(), dir.path()));
    let out = dir.path().join("audit.jsonl");
    let (code, ..) = text(&agentic(
        dir.path(),
        &["audit", "export", p(dir.path()), "--out", p(&out)],
    ));
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

    let (code, stdout, stderr) = text(&agentic(dir.path(), &["audit", "verify", p(&out)]));
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
    let (code, stdout, stderr) = text(&agentic(dir.path(), &["audit", "verify", p(&file)]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        format!("{stdout}{stderr}").contains("line 1"),
        "{stdout}{stderr}"
    );
}

#[test]
fn export_refuses_a_database_newer_than_the_binary() {
    let dir = tempfile::tempdir().unwrap();
    let layout = project(dir.path(), dir.path());
    am_store::testing::database_at_version(&layout.state_db(), SCHEMA_VERSION + 1).unwrap();
    let (code, stdout, stderr) = text(&agentic(dir.path(), &["audit", "export", p(dir.path())]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stdout.is_empty(), "{stdout}");
    assert!(stderr.contains("newer than this app knows"), "{stderr}");
}

#[test]
fn a_refused_export_leaves_an_existing_out_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    let layout = project(dir.path(), &root);
    am_store::testing::database_at_version(&layout.state_db(), SCHEMA_VERSION + 1).unwrap();
    let out = dir.path().join("audit.jsonl");
    std::fs::write(&out, "an earlier export\n").unwrap();
    let (code, stdout, stderr) = text(&agentic(
        dir.path(),
        &["audit", "export", p(&root), "--out", p(&out)],
    ));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("newer than this app knows"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        "an earlier export\n"
    );
    let left: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.'))
        .collect();
    assert_eq!(left.len(), 2, "no temporary file left: {left:?}");
}

#[test]
fn out_inside_the_runtime_folder_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    drop(populated(dir.path(), dir.path()));
    let id = am_config::read_company(dir.path())
        .unwrap()
        .project_id
        .unwrap();
    let layout = base(dir.path()).project(&id);
    let db = layout.state_db();
    let before = std::fs::read(&db).unwrap();
    for target in [db.clone(), layout.dir().join("audit.jsonl")] {
        let (code, stdout, stderr) = text(&agentic(
            dir.path(),
            &["audit", "export", p(dir.path()), "--out", p(&target)],
        ));
        assert_eq!(code, 2, "{target:?}: {stdout}{stderr}");
        assert!(stderr.contains("runtime folder"), "{stderr}");
    }
    assert_eq!(std::fs::read(&db).unwrap(), before, "state.db untouched");
    assert!(!layout.dir().join("audit.jsonl").exists());
}

/// The read-only resolve (story 1.13): no company.yaml, no project_id, an
/// unmoved legacy folder or no runtime folder on this Mac is refused in
/// words, and nothing is created.
#[test]
fn export_of_a_folder_without_a_project_on_this_mac_fails_in_words() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    let (code, stdout, stderr) = text(&agentic(dir.path(), &["audit", "export", p(&empty)]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("wizard"), "{stderr}");

    let no_id = dir.path().join("no-id");
    std::fs::create_dir(&no_id).unwrap();
    std::fs::write(no_id.join("company.yaml"), "schema_version: 1\n").unwrap();
    let (code, _, stderr) = text(&agentic(dir.path(), &["audit", "export", p(&no_id)]));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("no project_id"), "{stderr}");

    let elsewhere = dir.path().join("elsewhere");
    let layout = project(dir.path(), &elsewhere);
    std::fs::remove_dir(layout.dir()).unwrap();
    let (code, _, stderr) = text(&agentic(dir.path(), &["audit", "export", p(&elsewhere)]));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("no runtime folder on this Mac"), "{stderr}");

    std::fs::create_dir(legacy_dir(&elsewhere)).unwrap();
    let (code, _, stderr) = text(&agentic(dir.path(), &["audit", "export", p(&elsewhere)]));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("old runtime folder"), "{stderr}");
    assert!(!layout.dir().exists(), "nothing is created");
}

#[test]
fn bad_usage_exits_two() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        vec!["audit"],
        vec!["audit", "frobnicate"],
        vec!["audit", "export"],
        vec!["audit", "export", "/tmp", "--out"],
        vec!["audit", "verify"],
        vec!["audit", "verify", "a.jsonl", "b.jsonl"],
    ] {
        let (code, stdout, stderr) = text(&agentic(dir.path(), &args));
        assert_eq!(code, 2, "{args:?}: {stdout}{stderr}");
        assert!(
            stderr.contains("usage: agentic audit"),
            "{args:?}: {stderr}"
        );
    }
}
