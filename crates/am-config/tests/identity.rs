//! Story 1.13 acceptance tests (am-config side): the project id in
//! `company.yaml` (AD-32). `ConfigIdentityAssign` inserts one `project_id`
//! line after `schema_version` and leaves every other byte as it was, with
//! one History entry through the `ConfigHistory` port; `validate --template`
//! refuses an id; knowledge entries are checked against the folder the
//! caller names (a runtime folder, `runtime-seed/`, or none); the resolver's
//! reads of `company.yaml` and of the decision lock (a stdio MCP server or a
//! Source with `render: true`) need no store.
#![forbid(unsafe_code)]

mod common;

use std::path::Path;
use std::sync::Arc;

use am_config::{
    CompanyRefusal, ConfigIdentityAssign, ConfigWriter, LoadOptions, Severity, WriteError,
    decisions_locked, load, load_in, read_company,
};
use am_domain::{Actor, ConfigHistory, HistoryKind, ProjectId};
use am_testkit::InMemoryConfigHistory;
use common::{at, copy_of, fixture, write};

fn runtime() -> Actor {
    Actor::System {
        component: "runtime".into(),
    }
}

fn writer(root: &Path) -> (ConfigWriter, Arc<InMemoryConfigHistory>) {
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    (w, history)
}

/// A company.yaml with comments, blank lines, an unusual key order and a
/// trailing comment; the writer must keep every byte of it.
const COMMENTED: &str = "# The company file, written by hand.\nschema_version: 2  # keep this first\n\nname: Example Project   \ncurrency: EUR\n# the zone\ntimezone: UTC\nteams:\n  - id: core\n    name: Core team\n# end\n";

#[test]
fn assigning_an_id_inserts_one_line_and_keeps_every_other_byte() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "company.yaml", COMMENTED);
    let (w, history) = writer(root);
    let id = ProjectId::mint();

    let entry = w
        .assign_identity(
            runtime(),
            ConfigIdentityAssign {
                project_id: id.to_string(),
            },
        )
        .unwrap();

    let want = COMMENTED.replacen(
        "schema_version: 2  # keep this first\n",
        &format!("schema_version: 2  # keep this first\nproject_id: {id}\n"),
        1,
    );
    assert_eq!(
        std::fs::read_to_string(root.join("company.yaml")).unwrap(),
        want
    );
    // One History entry by System(runtime); nothing pending.
    let entries = history.entries(None).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, entry.id);
    assert_eq!(entries[0].kind, HistoryKind::Edit);
    assert_eq!(entries[0].actor, runtime());
    assert_eq!(entries[0].file, "company.yaml");
    assert!(entries[0].diff.contains(&format!("+project_id: {id}")));
    assert!(history.pending_intents().unwrap().is_empty());
    // The id reads back, and the project still validates.
    let head = read_company(root).unwrap();
    assert_eq!(head.project_id, Some(id));
    let loaded = load(root).unwrap();
    assert_eq!(loaded.error_count(), 0, "{:#?}", loaded.findings);
}

#[test]
fn a_crlf_file_gets_a_crlf_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let text = "schema_version: 1\r\nname: Example\r\ntimezone: UTC\r\ncurrency: EUR\r\n";
    write(root, "company.yaml", text);
    let (w, _) = writer(root);
    let id = ProjectId::mint();
    w.assign_identity(
        runtime(),
        ConfigIdentityAssign {
            project_id: id.to_string(),
        },
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("company.yaml")).unwrap(),
        format!(
            "schema_version: 1\r\nproject_id: {id}\r\nname: Example\r\ntimezone: UTC\r\ncurrency: EUR\r\n"
        )
    );
}

#[test]
fn an_id_already_present_or_a_bad_id_is_refused_and_nothing_is_written() {
    let dir = copy_of("valid");
    let root = dir.path();
    let id = ProjectId::mint();
    let (w, history) = writer(root);
    let bad = w
        .assign_identity(
            runtime(),
            ConfigIdentityAssign {
                project_id: "not-an-id".into(),
            },
        )
        .unwrap_err();
    assert!(matches!(bad, WriteError::BadEdit(_)), "{bad:?}");
    w.assign_identity(
        runtime(),
        ConfigIdentityAssign {
            project_id: id.to_string(),
        },
    )
    .unwrap();
    let before = std::fs::read(root.join("company.yaml")).unwrap();
    let again = w
        .assign_identity(
            runtime(),
            ConfigIdentityAssign {
                project_id: ProjectId::mint().to_string(),
            },
        )
        .unwrap_err();
    assert!(again.to_string().contains("already"), "{again}");
    assert_eq!(std::fs::read(root.join("company.yaml")).unwrap(), before);
    assert_eq!(history.entries(None).unwrap().len(), 1);
}

#[test]
fn validate_accepts_an_id_in_a_project_and_refuses_one_in_a_template() {
    let dir = copy_of("valid");
    let root = dir.path();
    let id = ProjectId::mint();
    // After the schema_version line, whatever version the fixture is at.
    let text = std::fs::read_to_string(root.join("company.yaml")).unwrap();
    let version = text
        .lines()
        .find(|l| l.starts_with("schema_version:"))
        .unwrap()
        .to_string();
    common::edit(
        root,
        "company.yaml",
        &format!("{version}\n"),
        &format!("{version}\nproject_id: {id}\n"),
    );
    let project = load(root).unwrap();
    assert_eq!(project.error_count(), 0, "{:#?}", project.findings);

    let template = load_in(root, LoadOptions::template(root)).unwrap();
    let f = at(&template.findings, "company.yaml", 2, "project_id");
    assert_eq!(f.severity, Severity::Error);
    assert!(f.fix.contains("remove"), "{}", f.fix);

    common::edit(root, "company.yaml", &id.to_string(), "not-an-id");
    let broken = load(root).unwrap();
    let f = at(&broken.findings, "company.yaml", 2, "project_id");
    assert_eq!(f.severity, Severity::Error);
}

#[test]
fn knowledge_is_checked_against_the_folder_the_caller_names() {
    let anatomy = fixture("anatomy");
    // A template: against runtime-seed/; the moved seeds are found and the
    // absent file is a warning.
    let template = load_in(&anatomy, LoadOptions::template(&anatomy)).unwrap();
    assert!(
        common::in_file(&template.findings, "seats/full.md").is_empty(),
        "{:#?}",
        template.findings
    );
    let f = at(&template.findings, "seats/soft.md", 7, "absent.md");
    assert_eq!(f.severity, Severity::Warning);
    assert!(f.message.contains("runtime-seed"), "{}", f.message);

    // A project with a runtime folder: against it.
    let rt = tempfile::tempdir().unwrap();
    let opts = LoadOptions::project(Some(rt.path().join("knowledge")));
    let with = load_in(&anatomy, opts).unwrap();
    at(&with.findings, "seats/full.md", 22, "guide.md");
    std::fs::create_dir_all(rt.path().join("knowledge/full")).unwrap();
    std::fs::write(rt.path().join("knowledge/full/guide.md"), "g").unwrap();
    std::fs::write(rt.path().join("knowledge/full/terms.txt"), "t").unwrap();
    let opts = LoadOptions::project(Some(rt.path().join("knowledge")));
    let found = load_in(&anatomy, opts).unwrap();
    assert!(
        common::in_file(&found.findings, "seats/full.md").is_empty(),
        "{:#?}",
        found.findings
    );

    // Without one, the existence warning is skipped.
    let none = load(&anatomy).unwrap();
    assert!(
        !none
            .findings
            .iter()
            .any(|f| f.file == "seats/soft.md" && f.message.contains("absent.md")),
        "{:#?}",
        none.findings
    );
}

#[test]
fn the_resolver_reads_company_yaml_in_words() {
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(
        read_company(empty.path()).unwrap_err(),
        CompanyRefusal::Empty
    );

    let config_only = tempfile::tempdir().unwrap();
    write(config_only.path(), "tools.yaml", "schema_version: 1\n");
    let err = read_company(config_only.path()).unwrap_err();
    assert_eq!(err, CompanyRefusal::NoCompanyYaml);
    assert!(err.to_string().contains("no company.yaml"), "{err}");

    // Story 1.14: this app reads schema_version 2, so 3 is newer.
    let newer = tempfile::tempdir().unwrap();
    write(newer.path(), "company.yaml", "schema_version: 3\nname: X\n");
    let err = read_company(newer.path()).unwrap_err();
    assert!(
        matches!(err, CompanyRefusal::TooNew { found: 3, known: 2 }),
        "{err:?}"
    );
    assert!(err.to_string().contains("update the app"), "{err}");

    let broken = tempfile::tempdir().unwrap();
    write(broken.path(), "company.yaml", "schema_version: [1\n");
    let err = read_company(broken.path()).unwrap_err();
    assert!(matches!(err, CompanyRefusal::Unreadable(_)), "{err:?}");

    let ok = copy_of("valid");
    let head = read_company(ok.path()).unwrap();
    assert_eq!(head.project_id, None);
}

#[test]
fn decisions_are_locked_by_a_stdio_mcp_server_or_a_rendering_source() {
    // The valid fixture enables a stdio MCP server.
    let valid = copy_of("valid");
    let why = decisions_locked(valid.path()).expect("locked");
    assert!(why.contains("notes"), "{why}");

    let plain = tempfile::tempdir().unwrap();
    write(
        plain.path(),
        "company.yaml",
        "schema_version: 1\nname: X\ntimezone: UTC\ncurrency: EUR\n",
    );
    assert_eq!(decisions_locked(plain.path()), None);

    let render = copy_of("sources-ok");
    let reader = std::fs::read_dir(render.path().join("sources"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            std::fs::read_to_string(p)
                .unwrap()
                .contains("type: http-reader")
        })
        .expect("sources-ok has an http-reader Source");
    let text = std::fs::read_to_string(&reader).unwrap();
    if !text.contains("render: true") {
        std::fs::write(&reader, format!("{text}render: true\n")).unwrap();
    }
    let why = decisions_locked(render.path()).expect("locked");
    assert!(why.contains("render"), "{why}");
}
