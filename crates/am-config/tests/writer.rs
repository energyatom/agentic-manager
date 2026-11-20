//! Story 1.8 acceptance tests (am-config side): the config writer, its
//! canonical emitter, History through the `ConfigHistory` port (faked by
//! am-testkit's `InMemoryConfigHistory`), Undo, Restore and the crash rules.
//! Written before the writer exists. The same rows run against the real store
//! in `crates/am-e2e/tests/config_history.rs`.
#![forbid(unsafe_code)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use am_config::{
    ConfigEdit, ConfigSnapshot, ConfigWriter, EditOp, FailPoint, WriteError, canonical_bytes,
    config_files, load,
};
use am_domain::{Actor, ConfigHistory, HistoryKind};
use am_testkit::InMemoryConfigHistory;
use common::{copy_dir, copy_of, snapshot};
use serde_json::json;
use sha2::{Digest, Sha256};

const SEAT: &str = "seats/alpha.md";

/// alpha.md with its frontmatter keys out of canonical order and a body whose
/// bytes (trailing spaces, a tab, a `---` line, no final newline) must survive.
const SHUFFLED: &str = "---\nbudget:\n  per_week: 10\n  per_run: 1.5\ntools:\n  - web_search\n  - table_read\n  - notes/list_notes\nteam: core\nskills:\n  - brief\nname: Alpha\ncapabilities:\n  web_search:\n    provider: builtin\n    on: true\n  tables:\n    on: true\nschema_version: 1\nmodel:\n  model: example-local\n  account: local-lmstudio\nrole: Analyst\ndescription: Gathers the new items.\n---\nRead the items added since the last run.  \n\tKeep the list short.\n---\nno final newline";
const BODY: &str =
    "Read the items added since the last run.  \n\tKeep the list short.\n---\nno final newline";

fn reviewer() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Reviewer".into(),
    }
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn read(root: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(root.join(rel)).unwrap()
}

fn set(path: &[&str], value: serde_json::Value) -> EditOp {
    EditOp::Set {
        path: path.iter().map(|s| s.to_string()).collect(),
        value,
    }
}

fn edit(file: &str, ops: Vec<EditOp>) -> ConfigEdit {
    ConfigEdit {
        file: file.into(),
        ops,
        body: None,
    }
}

fn writer(root: &Path) -> (ConfigWriter, Arc<InMemoryConfigHistory>) {
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    (w, history)
}

/// The top-level keys of a seat file's frontmatter, in file order.
fn front_keys(text: &str) -> Vec<String> {
    let front = text
        .strip_prefix("---\n")
        .and_then(|t| t.split_once("\n---\n"))
        .map(|(f, _)| f)
        .unwrap();
    front
        .lines()
        .filter(|l| !l.starts_with(' ') && !l.starts_with('-') && l.contains(':'))
        .map(|l| l.split(':').next().unwrap().to_string())
        .collect()
}

/// The snapshot's values without its content hash (and a Source's verbatim
/// file text, which is the file's bytes by definition).
fn values(s: &ConfigSnapshot) -> String {
    let mut sources = s.sources().clone();
    for src in sources.values_mut() {
        src.text.clear();
    }
    format!(
        "{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}",
        s.company(),
        s.accounts(),
        s.prices(),
        s.capabilities(),
        s.tools(),
        s.seats(),
        s.workflows(),
        s.tables(),
        s.schemas(),
        s.dashboards(),
        sources,
        s.skills()
    )
}

// ------------------------------------------------------------- matrix rows

#[test]
fn writer_edit_seat_is_canonical_keeps_the_body_and_records_history() {
    let dir = copy_of("valid");
    let root = dir.path();
    std::fs::write(root.join(SEAT), SHUFFLED).unwrap();
    let before = read(root, SEAT);
    let (w, history) = writer(root);

    let entry = w
        .apply(
            reviewer(),
            edit(SEAT, vec![set(&["budget", "per_run"], json!(2))]),
        )
        .unwrap();

    let after = read(root, SEAT);
    let text = String::from_utf8(after.clone()).unwrap();
    assert_eq!(
        front_keys(&text),
        [
            "schema_version",
            "name",
            "role",
            "team",
            "description",
            "model",
            "capabilities",
            "skills",
            "tools",
            "budget"
        ],
        "{text}"
    );
    // Nested keys are sorted; sequences keep their order.
    assert!(
        text.contains("model:\n  account: local-lmstudio\n  model: example-local\n"),
        "{text}"
    );
    assert!(
        text.contains("capabilities:\n  tables:\n    on: true\n  web_search:\n    on: true\n    provider: builtin\n"),
        "{text}"
    );
    assert!(
        text.contains("tools:\n  - web_search\n  - table_read\n  - notes/list_notes\n"),
        "{text}"
    );
    assert!(
        text.contains("budget:\n  per_run: 2\n  per_week: 10\n"),
        "{text}"
    );
    assert!(text.ends_with(&format!("\n---\n{BODY}")), "{text}");

    // The project re-loads with no errors and the new value.
    let snap = snapshot(root);
    let old = snapshot(&common::fixture("valid")).seats()["alpha"].clone();
    let new = &snap.seats()["alpha"];
    assert_ne!(new.budget, old.budget);
    assert_eq!(
        new.budget.as_ref().unwrap().per_week,
        old.budget.unwrap().per_week
    );

    // One History entry: diff, Actor and hat in words, both byte strings.
    assert_eq!(entry.kind, HistoryKind::Edit);
    assert_eq!(entry.file, SEAT);
    assert_eq!(entry.who(), "You (Reviewer)");
    assert_eq!(entry.ref_entry, None);
    assert_eq!(entry.before, before);
    assert_eq!(entry.after, after);
    assert_eq!(entry.before_sha256, sha(&before));
    assert_eq!(entry.after_sha256, sha(&after));
    assert!(
        entry
            .diff
            .starts_with("--- a/seats/alpha.md\n+++ b/seats/alpha.md\n@@ ")
    );
    assert!(entry.diff.contains("\n+  per_run: 2\n"), "{}", entry.diff);
    assert!(entry.diff.contains("\n-  per_run: 1.5\n"), "{}", entry.diff);
    assert_eq!(history.entries(None).unwrap(), vec![entry.clone()]);
    assert!(history.pending_intents().unwrap().is_empty());
    assert!(w.is_own_write(SEAT, &entry.after_sha256));
    assert!(!w.is_own_write(SEAT, &entry.before_sha256));
}

#[test]
fn writer_undo_writes_the_old_bytes_back_as_a_new_entry() {
    let dir = copy_of("valid");
    let root = dir.path();
    let before = read(root, SEAT);
    let (w, history) = writer(root);
    let e1 = w
        .apply(
            reviewer(),
            edit(SEAT, vec![set(&["budget", "per_run"], json!(2))]),
        )
        .unwrap();

    let owner = Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    };
    let e2 = w.undo(owner, &e1.id).unwrap();

    assert_eq!(read(root, SEAT), before, "the old bytes come back exactly");
    assert_eq!(e2.kind, HistoryKind::Undo);
    assert_eq!(e2.ref_entry.as_deref(), Some(e1.id.as_str()));
    assert_eq!(e2.who(), "You (Owner)");
    assert_eq!(e2.before, e1.after);
    assert_eq!(e2.after, before);
    assert!(e2.seq > e1.seq);
    assert_eq!(history.entries(Some(SEAT.into())).unwrap().len(), 2);
}

#[test]
fn writer_undo_is_refused_when_the_file_changed_since() {
    let dir = copy_of("valid");
    let root = dir.path();
    let (w, history) = writer(root);
    let e1 = w
        .apply(
            reviewer(),
            edit(SEAT, vec![set(&["budget", "per_run"], json!(2))]),
        )
        .unwrap();
    // A later edit through the writer.
    w.apply(
        reviewer(),
        edit(SEAT, vec![set(&["budget", "per_week"], json!(12))]),
    )
    .unwrap();
    let now = read(root, SEAT);
    let err = w.undo(reviewer(), &e1.id).unwrap_err();
    assert!(matches!(err, WriteError::ChangedSince { .. }), "{err:?}");
    assert!(
        err.to_string().contains("changed since this entry"),
        "{err}"
    );
    assert_eq!(read(root, SEAT), now, "nothing written");
    assert_eq!(history.entries(None).unwrap().len(), 2);

    // An outside edit after the latest entry refuses its Undo too.
    let e3 = history.entries(None).unwrap().pop().unwrap();
    let outside = format!("{}\n", String::from_utf8(now).unwrap());
    std::fs::write(root.join(SEAT), &outside).unwrap();
    let err = w.undo(reviewer(), &e3.id).unwrap_err();
    assert!(
        err.to_string().contains("changed since this entry"),
        "{err}"
    );
    assert_eq!(read(root, SEAT), outside.as_bytes());
    assert!(history.pending_intents().unwrap().is_empty());
}

#[test]
fn writer_restore_writes_an_older_entrys_bytes_as_a_new_entry() {
    let dir = copy_of("valid");
    let root = dir.path();
    let (w, _history) = writer(root);
    let e1 = w
        .apply(
            reviewer(),
            edit(SEAT, vec![set(&["budget", "per_run"], json!(2))]),
        )
        .unwrap();
    let _e2 = w
        .apply(
            reviewer(),
            edit(SEAT, vec![set(&["budget", "per_run"], json!(3))]),
        )
        .unwrap();
    let e3 = w
        .apply(
            reviewer(),
            edit(SEAT, vec![set(&["budget", "per_week"], json!(20))]),
        )
        .unwrap();

    let e4 = w.restore(reviewer(), &e1.id).unwrap();
    assert_eq!(read(root, SEAT), e1.after);
    assert_eq!(e4.kind, HistoryKind::Restore);
    assert_eq!(e4.ref_entry.as_deref(), Some(e1.id.as_str()));
    assert_eq!(e4.before, e3.after);
    assert_eq!(e4.after, e1.after);

    // Restoring what the file already holds is refused in words.
    let err = w.restore(reviewer(), &e1.id).unwrap_err();
    assert!(matches!(err, WriteError::NoChange { .. }), "{err:?}");
    assert!(err.to_string().contains("already"), "{err}");

    let err = w.restore(reviewer(), "no-such-entry").unwrap_err();
    assert!(matches!(err, WriteError::UnknownEntry(_)), "{err:?}");
}

#[test]
fn writer_refuses_an_invalid_result_with_file_line_and_fix() {
    let dir = copy_of("valid");
    let root = dir.path();
    let before = read(root, SEAT);
    let (w, history) = writer(root);
    let err = w
        .apply(
            reviewer(),
            edit(SEAT, vec![set(&["team"], json!("no-such-team"))]),
        )
        .unwrap_err();
    let WriteError::Invalid { findings, .. } = &err else {
        panic!("{err:?}")
    };
    let f = findings
        .iter()
        .find(|f| f.file == SEAT && f.message.contains("no-such-team"))
        .unwrap_or_else(|| panic!("{findings:#?}"));
    assert!(f.line > 1);
    assert!(!f.fix.is_empty());
    let words = err.to_string();
    assert!(words.contains(&format!("{SEAT}:{}", f.line)), "{words}");
    assert!(words.contains("fix:"), "{words}");
    assert_eq!(read(root, SEAT), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

#[test]
fn writer_refuses_bad_paths_in_words() {
    let dir = copy_of("valid");
    let root = dir.path();
    let (w, history) = writer(root);
    let outside = root.parent().unwrap().join("x.yaml");
    for file in [
        "../x.yaml",
        "/etc/x",
        outside.to_str().unwrap(),
        "seats/../company.yaml",
        "seats/missing.md",
        "notes.txt",
        "skills/brief/references/style.md",
        ".agentic/state.db",
        "seats/.alpha.md.am-tmp",
        "",
    ] {
        let err = w
            .apply(reviewer(), edit(file, vec![set(&["name"], json!("X"))]))
            .unwrap_err();
        assert!(matches!(err, WriteError::BadPath { .. }), "{file}: {err:?}");
        assert!(err.to_string().contains("refused"), "{file}: {err}");
    }
    assert!(!outside.exists());
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

#[test]
fn writer_body_is_replaced_only_when_asked_and_only_in_markdown() {
    let dir = copy_of("valid");
    let root = dir.path();
    let (w, _h) = writer(root);
    w.apply(
        reviewer(),
        ConfigEdit {
            file: SEAT.into(),
            ops: vec![],
            body: Some("New instructions.\n".into()),
        },
    )
    .unwrap();
    let text = String::from_utf8(read(root, SEAT)).unwrap();
    assert!(text.ends_with("\n---\nNew instructions.\n"), "{text}");
    assert_eq!(
        snapshot(root).seats()["alpha"].instructions,
        "New instructions.\n"
    );

    let err = w
        .apply(
            reviewer(),
            ConfigEdit {
                file: "company.yaml".into(),
                ops: vec![],
                body: Some("x".into()),
            },
        )
        .unwrap_err();
    assert!(matches!(err, WriteError::BadEdit(_)), "{err:?}");
}

#[test]
fn writer_edits_nested_paths_lists_and_removals() {
    let dir = copy_of("valid");
    let root = dir.path();
    let (w, _h) = writer(root);
    w.apply(
        reviewer(),
        edit(
            "company.yaml",
            vec![
                set(&["teams", "1", "name"], json!("Help desk")),
                EditOp::Remove {
                    path: vec!["description".into()],
                },
            ],
        ),
    )
    .unwrap();
    let snap = snapshot(root);
    assert_eq!(snap.company().teams[1].name, "Help desk");
    assert_eq!(snap.company().description, None);

    // A path through a scalar is refused before anything is written.
    let before = read(root, "company.yaml");
    let err = w
        .apply(
            reviewer(),
            edit("company.yaml", vec![set(&["name", "x"], json!(1))]),
        )
        .unwrap_err();
    assert!(matches!(err, WriteError::BadEdit(_)), "{err:?}");
    assert_eq!(read(root, "company.yaml"), before);
}

// --------------------------------------------------------------- crash rules

#[test]
fn writer_interrupted_before_rename_leaves_the_file_and_no_temp() {
    let dir = copy_of("valid");
    let root = dir.path();
    let before = read(root, SEAT);
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    w.fail_at(Some(FailPoint::BeforeRename));
    let err = w
        .apply(
            reviewer(),
            edit(SEAT, vec![set(&["budget", "per_run"], json!(2))]),
        )
        .unwrap_err();
    assert!(matches!(err, WriteError::Interrupted(_)), "{err:?}");
    assert_eq!(read(root, SEAT), before, "the original is intact");
    assert!(
        root.join("seats/.alpha.md.am-tmp").exists(),
        "the crash left its temp file"
    );
    assert_eq!(history.pending_intents().unwrap().len(), 1);
    drop(w);

    // The next writer start removes the temp file; reconcile drops the intent.
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    assert!(!root.join("seats/.alpha.md.am-tmp").exists());
    let report = w
        .reconcile(Actor::System {
            component: "config-writer".into(),
        })
        .unwrap();
    assert!(report.completed.is_empty());
    assert_eq!(report.dropped.len(), 1);
    assert!(history.pending_intents().unwrap().is_empty());
    assert!(history.entries(None).unwrap().is_empty());
    assert_eq!(read(root, SEAT), before);
}

#[test]
fn writer_crash_after_rename_is_completed_by_reconcile() {
    let dir = copy_of("valid");
    let root = dir.path();
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    w.fail_at(Some(FailPoint::AfterRename));
    let err = w
        .apply(
            reviewer(),
            edit(SEAT, vec![set(&["budget", "per_run"], json!(2))]),
        )
        .unwrap_err();
    assert!(matches!(err, WriteError::Interrupted(_)), "{err:?}");
    let written = read(root, SEAT);
    assert!(history.entries(None).unwrap().is_empty());
    drop(w);

    let w = ConfigWriter::open(root, history.clone()).unwrap();
    let report = w
        .reconcile(Actor::System {
            component: "config-writer".into(),
        })
        .unwrap();
    assert!(report.dropped.is_empty());
    assert_eq!(report.completed.len(), 1);
    let entry = &report.completed[0];
    assert_eq!(entry.after, written);
    assert_eq!(
        entry.who(),
        "You (Reviewer)",
        "the entry keeps the edit's Actor"
    );
    assert_eq!(history.entries(None).unwrap(), report.completed);
    assert!(history.pending_intents().unwrap().is_empty());
    assert!(w.is_own_write(SEAT, &entry.after_sha256));
}

#[test]
fn writer_runs_one_edit_at_a_time() {
    let dir = copy_of("valid");
    let root = dir.path();
    let (w, history) = writer(root);
    let w = Arc::new(w);
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let w = w.clone();
            std::thread::spawn(move || {
                w.apply(
                    reviewer(),
                    edit(SEAT, vec![set(&["budget", "per_week"], json!(20 + i))]),
                )
                .unwrap()
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let entries = history.entries(None).unwrap();
    assert_eq!(entries.len(), 8);
    // Each entry starts from the bytes the one before it left.
    for pair in entries.windows(2) {
        assert_eq!(pair[0].after_sha256, pair[1].before_sha256);
    }
    assert_eq!(sha(&read(root, SEAT)), entries[7].after_sha256);
}

// ------------------------------------------------------- canonical emission

fn template_projects() -> Vec<PathBuf> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = vec![repo.join("fixtures/projects/valid")];
    for group in ["departments", "roles", "samples"] {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(repo.join("templates").join(group))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        out.extend(dirs);
    }
    out
}

#[test]
fn writer_canonical_emit_is_stable_and_reloads_the_same_values() {
    let projects = template_projects();
    assert!(projects.len() >= 11, "{projects:?}");
    let mut emitted = 0;
    for project in projects {
        let original = values(&snapshot(&project));
        let dir = tempfile::tempdir().unwrap();
        copy_dir(&project, dir.path());
        let files = config_files(dir.path());
        assert!(files.iter().any(|f| f == "company.yaml"), "{files:?}");
        for rel in &files {
            let bytes = read(dir.path(), rel);
            let once = canonical_bytes(rel, &bytes)
                .unwrap_or_else(|e| panic!("{}/{rel}: {e}", project.display()));
            let twice = canonical_bytes(rel, &once).unwrap();
            assert_eq!(
                String::from_utf8_lossy(&once),
                String::from_utf8_lossy(&twice),
                "{}/{rel}: a second emit changed the bytes",
                project.display()
            );
            std::fs::write(dir.path().join(rel), &once).unwrap();
            emitted += 1;
        }
        let loaded = load(dir.path()).unwrap();
        let errors: Vec<_> = loaded
            .findings
            .iter()
            .filter(|f| f.severity == am_config::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{}: {errors:#?}", project.display());
        assert_eq!(
            values(loaded.snapshot.as_ref().unwrap()),
            original,
            "{}: canonical files load to other values",
            project.display()
        );
    }
    assert!(emitted > 30, "{emitted}");
}

#[test]
fn writer_config_files_lists_every_known_config_file_only() {
    let dir = copy_of("valid");
    let files = config_files(dir.path());
    assert_eq!(
        files,
        [
            "company.yaml",
            "dashboards/overview.yaml",
            "models/accounts.yaml",
            "models/capabilities.yaml",
            "models/prices.yaml",
            "schemas/summary.json",
            "seats/alpha.md",
            "seats/beta.md",
            "skills/brief/SKILL.md",
            "sources/feed.yaml",
            "tables/items.yaml",
            "tools.yaml",
            "workflows/digest.yaml",
        ]
    );
}

// ------------------------------------------------- review pass 1 (patches)

/// Review 1: a link at the temp path never sends the write outside the
/// project; a writer start removes such a link.
#[cfg(unix)]
#[test]
fn writer_never_follows_a_link_at_the_temp_path() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let dir = copy_of("valid");
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("target.txt");
    std::fs::write(&target, "outside").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    let tmp = root.join("seats/.alpha.md.am-tmp");
    symlink(&target, &tmp).unwrap();

    let (w, _h) = writer(root);
    assert!(
        std::fs::symlink_metadata(&tmp).is_err(),
        "a writer start removes a temp-named link"
    );

    // A link that appears later is not followed either.
    symlink(&target, &tmp).unwrap();
    w.apply(
        reviewer(),
        edit(SEAT, vec![set(&["budget", "per_run"], json!(2))]),
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "outside");
    let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "the link's target keeps its mode");
    let seat = std::fs::symlink_metadata(root.join(SEAT)).unwrap();
    assert!(
        seat.file_type().is_file(),
        "the seat is a file, not the link"
    );
    assert!(std::fs::symlink_metadata(&tmp).is_err());
    assert!(
        String::from_utf8(read(root, SEAT))
            .unwrap()
            .contains("per_run: 2\n")
    );
}

/// Review 2: a name that matches the file only by case is refused (the
/// exact-name check itself is unit-tested in `writer.rs`, independent of the
/// file system's case behaviour).
#[test]
fn writer_refuses_a_name_that_is_not_the_exact_name_on_disk() {
    let dir = copy_of("valid");
    let root = dir.path();
    let before = read(root, SEAT);
    let (w, history) = writer(root);
    let err = w
        .apply(
            reviewer(),
            edit(
                "seats/ALPHA.md",
                vec![set(&["team"], json!("no-such-team"))],
            ),
        )
        .unwrap_err();
    assert!(matches!(err, WriteError::BadPath { .. }), "{err:?}");
    assert_eq!(read(root, SEAT), before, "nothing written");
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());
}

/// Review 3: an untouched scalar keeps its text when it differs from the
/// typed value's rendering (`1.10`, `007`); a value an edit sets is written
/// from the new value.
#[test]
fn writer_keeps_the_text_of_untouched_number_written_values() {
    let dir = copy_of("valid");
    let root = dir.path();
    let skill = "skills/brief/SKILL.md";
    std::fs::write(
        root.join(skill),
        "---\nname: brief\ndescription: Writes a short brief from the gathered items.\nlicense: MIT\nmetadata:\n  version: 1.10\n  build: 007\n  ratio: 1.5\nallowed-tools: read_website\n---\nKeep each brief short.\n",
    )
    .unwrap();
    let original = snapshot(root).skills()["brief"].metadata.clone();
    assert_eq!(original["version"], "1.10");
    assert_eq!(original["build"], "007");

    let once = canonical_bytes(skill, &read(root, skill)).unwrap();
    let text = String::from_utf8(once.clone()).unwrap();
    assert!(
        text.contains("metadata:\n  build: 007\n  ratio: 1.5\n  version: 1.10\n"),
        "{text}"
    );
    assert_eq!(canonical_bytes(skill, &once).unwrap(), once);

    let (w, _h) = writer(root);
    w.apply(
        reviewer(),
        edit(skill, vec![set(&["description"], json!("Writes a brief."))]),
    )
    .unwrap();
    let text = String::from_utf8(read(root, skill)).unwrap();
    assert!(text.contains("  build: 007\n"), "{text}");
    assert!(text.contains("  version: 1.10\n"), "{text}");
    assert_eq!(snapshot(root).skills()["brief"].metadata, original);

    w.apply(
        reviewer(),
        edit(skill, vec![set(&["metadata", "version"], json!(2.50))]),
    )
    .unwrap();
    let text = String::from_utf8(read(root, skill)).unwrap();
    assert!(text.contains("  version: 2.5\n"), "{text}");
    assert!(text.contains("  build: 007\n"), "{text}");
}

/// Review 4: a secret-looking value in the file as it is now (frontmatter or
/// body) refuses the write; nothing reaches an intent, History or a diff.
#[test]
fn writer_refuses_while_the_file_holds_a_secret_looking_value() {
    const KEY: &str = "sk-ant-canary0canary0canary";
    for (from, to) in [
        (
            "description: Gathers the new items.",
            format!("description: {KEY}"),
        ),
        (
            "Read the items added",
            format!("Use {KEY}.\nRead the items added"),
        ),
    ] {
        let dir = copy_of("valid");
        let root = dir.path();
        common::edit(root, SEAT, from, &to);
        let before = read(root, SEAT);
        let (w, history) = writer(root);
        let err = w
            .apply(
                reviewer(),
                edit(
                    SEAT,
                    vec![set(&["description"], json!("Gathers the new items."))],
                ),
            )
            .unwrap_err();
        let words = err.to_string();
        assert!(words.contains("secret-looking"), "{words}");
        assert!(words.contains("by hand"), "{words}");
        assert!(!words.contains(KEY), "{words}");
        assert_eq!(read(root, SEAT), before, "nothing written");
        assert!(history.entries(None).unwrap().is_empty());
        assert!(history.pending_intents().unwrap().is_empty());
    }
}

/// Review 5: an intent left pending while the writer runs is settled before
/// the next write of its file, as `reconcile` would at the next start.
#[test]
fn writer_settles_a_pending_intent_of_the_file_before_the_next_write() {
    let dir = copy_of("valid");
    let root = dir.path();
    let (w, history) = writer(root);
    let per_run = |v: i64| edit(SEAT, vec![set(&["budget", "per_run"], json!(v))]);

    // Renamed, History not committed: completed before the next write.
    w.fail_at(Some(FailPoint::AfterRename));
    w.apply(reviewer(), per_run(2)).unwrap_err();
    let renamed = read(root, SEAT);
    w.fail_at(None);
    let e2 = w.apply(reviewer(), per_run(3)).unwrap();
    assert!(history.pending_intents().unwrap().is_empty());
    let entries = history.entries(None).unwrap();
    assert_eq!(entries.len(), 2, "{entries:#?}");
    assert_eq!(entries[0].after, renamed);
    assert_eq!(entries[0].who(), "You (Reviewer)");
    assert_eq!(entries[1], e2);
    assert_eq!(e2.before, renamed);

    // Not renamed: dropped before the next write.
    w.fail_at(Some(FailPoint::BeforeRename));
    w.apply(reviewer(), per_run(4)).unwrap_err();
    w.fail_at(None);
    w.apply(reviewer(), per_run(5)).unwrap();
    assert!(history.pending_intents().unwrap().is_empty());
    assert_eq!(history.entries(None).unwrap().len(), 3);

    let report = w
        .reconcile(Actor::System {
            component: "config-writer".into(),
        })
        .unwrap();
    assert!(report.completed.is_empty() && report.dropped.is_empty());
}
