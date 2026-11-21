//! Story 1.13: resolving and opening a project (AD-32) in process, over
//! am-testkit's `FakeProjectIdentity` (a copied folder is another root; a
//! volume can be unmounted; a tool can fail) and a scripted prompt. One test
//! per row of the story's I/O matrix that needs the fake or a fail point;
//! the rows the owner runs end to end (`agentic serve --project`) are in
//! `serve_project.rs`, and the stub-tool adapter in `identity_tools.rs`.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use am_app::runtime::{
    Marker, OpenRequest, Opened, Places, Prompt, Refusal, find_runtime, open_project, read_mac_id,
    records,
};
use am_domain::runtime::{LEGACY_COPY_PREFIX, OPEN_MARKER, legacy_dir, open_marker};
use am_domain::{
    Actor, ConfigHistory, IdentityError, MacId, ProjectId, ProjectIdentity, RuntimeLayout,
};
use am_store::{SCHEMA_VERSION, StoreConfigHistory};
use am_testkit::{FakeProjectIdentity, copy_tree};

const COMPANY: &str = "schema_version: 1\nname: Example\ntimezone: UTC\ncurrency: EUR\n";
const COMMENTED: &str = "# Written by hand.\nschema_version: 1   # first\n\nname: Example   \n# zone\ntimezone: UTC\ncurrency: EUR\n";

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// A temporary HOME with its places, and a folder for projects.
struct Env {
    dir: tempfile::TempDir,
    places: Places,
    fake: FakeProjectIdentity,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let places = Places::under(&home.join("Library/Application Support"), &home);
    Env {
        dir,
        places,
        fake: FakeProjectIdentity::new("Test Mac"),
    }
}

impl Env {
    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    /// A project folder `name` (under `projects/`) holding `company`.
    fn project(&self, name: &str, company: &str) -> PathBuf {
        let root = self.dir.path().join("projects").join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("company.yaml"), company).unwrap();
        root.canonicalize().unwrap()
    }

    fn open(&self, root: &Path) -> Result<Opened, Refusal> {
        self.open_with(root, false, false, &mut Answers::none())
    }

    fn open_with(
        &self,
        root: &Path,
        confirm_move: bool,
        take_over: bool,
        prompt: &mut Answers,
    ) -> Result<Opened, Refusal> {
        let req = OpenRequest {
            root: root.to_path_buf(),
            confirm_move,
            take_over,
        };
        open_project(&self.places, &self.fake, &req, prompt)
    }

    fn layout(&self, id: &ProjectId) -> RuntimeLayout {
        self.places.base.project(id)
    }

    /// The runtime folders in the base.
    fn runtime_folders(&self) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(self.places.base.dir()) else {
            return Vec::new();
        };
        let mut v: Vec<String> = rd
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| ProjectId::parse(n).is_some())
            .collect();
        v.sort();
        v
    }
}

/// A scripted terminal.
struct Answers {
    interactive: bool,
    answers: Vec<bool>,
    asked: Vec<String>,
}

impl Answers {
    fn none() -> Answers {
        Answers {
            interactive: false,
            answers: vec![],
            asked: vec![],
        }
    }
    fn terminal(answers: &[bool]) -> Answers {
        Answers {
            interactive: true,
            answers: answers.to_vec(),
            asked: vec![],
        }
    }
}

impl Prompt for Answers {
    fn interactive(&self) -> bool {
        self.interactive
    }
    fn ask(&mut self, question: &str) -> bool {
        self.asked.push(question.to_string());
        if self.answers.is_empty() {
            false
        } else {
            self.answers.remove(0)
        }
    }
}

fn id_of(root: &Path) -> Option<ProjectId> {
    am_config::read_company(root).unwrap().project_id
}

/// The exported audit entries (JSON Lines, keys sorted) of an open project.
fn audit(o: &Opened) -> Vec<String> {
    let mut out = Vec::new();
    o.store
        .reader()
        .unwrap()
        .export_audit_jsonl(&mut out)
        .unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

fn actions(entries: &[String], action: &str) -> Vec<String> {
    let key = format!("\"action\":\"{action}\"");
    entries
        .iter()
        .filter(|e| e.contains(&key))
        .cloned()
        .collect()
}

const SYSTEM_RUNTIME: &str = r#""actor":{"component":"runtime","kind":"system"}"#;
const HUMAN_OWNER: &str = r#""actor":{"hat":"Owner","kind":"human","person":"owner"}"#;

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn err(r: Result<Opened, Refusal>) -> String {
    match r {
        Ok(_) => panic!("the open must be refused"),
        Err(e) => e.0,
    }
}

fn runtime() -> Actor {
    Actor::System {
        component: "runtime".into(),
    }
}

// ------------------------------------------------------------ new project

#[test]
fn a_new_project_gets_one_inserted_id_line_and_its_runtime_folder() {
    let e = env();
    let root = e.project("a", COMMENTED);
    let o = e.open(&root).unwrap();
    let id = o.project_id;

    // company.yaml: one line inserted after schema_version, nothing else.
    let want = COMMENTED.replacen(
        "schema_version: 1   # first\n",
        &format!("schema_version: 1   # first\nproject_id: {id}\n"),
        1,
    );
    assert_eq!(
        std::fs::read_to_string(root.join("company.yaml")).unwrap(),
        want
    );
    // One History entry, one config.edited audit, both by System(runtime).
    let history = StoreConfigHistory::new(&o.store).unwrap();
    let entries = history.entries(None).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].actor, runtime());
    let log = audit(&o);
    let edited = actions(&log, "config.edited");
    assert_eq!(edited.len(), 1, "{log:#?}");
    assert!(edited[0].contains(SYSTEM_RUNTIME), "{}", edited[0]);
    // No approved grant set is written (AD-16: an identity write commits
    // History and audit only).
    assert!(!log.iter().any(|l| l.contains("grant")), "{log:#?}");
    // The runtime folder: under the base, 0700, files 0600.
    let layout = e.layout(&id);
    assert_eq!(o.layout, layout);
    assert_eq!(mode(layout.dir()), 0o700);
    for f in [layout.lock(), layout.state_db(), layout.root_record()] {
        assert_eq!(mode(&f), 0o600, "{}", f.display());
    }
    assert!(names(layout.dir()).iter().all(|n| !n.starts_with('.')));
    let recs = records(&e.places.base);
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].project_id, id);
    assert_eq!(recs[0].root.path, root);
    let index = std::fs::read_to_string(e.places.base.root_index()).unwrap();
    assert!(index.contains(&id.to_string()), "{index}");
    // The project gains only the marker, which holds this Mac's random id.
    assert_eq!(names(&root), vec![OPEN_MARKER, "company.yaml"]);
    let marker = Marker::parse(&std::fs::read_to_string(open_marker(&root)).unwrap()).unwrap();
    let mac = read_mac_id(&e.places.app_data).unwrap().unwrap();
    assert_eq!(marker.mac_id, mac.to_string());
    assert_eq!(marker.mac_name, "Test Mac");
    assert_eq!(marker.pid, std::process::id());
    assert_eq!(mode(&e.places.app_data.join("mac-id")), 0o600);
    drop(o);
    assert_eq!(
        names(&root),
        vec!["company.yaml"],
        "the marker goes at close"
    );
}

#[test]
fn an_empty_folder_names_the_wizard_and_creates_nothing() {
    let e = env();
    let root = e.dir.path().join("empty");
    std::fs::create_dir(&root).unwrap();
    let msg = err(e.open(&root));
    assert!(msg.contains("first-run wizard"), "{msg}");
    assert!(!e.places.base.dir().exists(), "the base is not created");
    assert!(!e.places.app_data.exists());
    assert!(names(&root).is_empty());
}

#[test]
fn a_folder_with_config_files_but_no_company_yaml_is_refused() {
    let e = env();
    let root = e.dir.path().join("half");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("tools.yaml"), "schema_version: 1\n").unwrap();
    let msg = err(e.open(&root));
    assert!(msg.contains("no company.yaml"), "{msg}");
    assert!(!msg.contains("wizard"), "{msg}");
    assert!(!e.places.base.dir().exists());
}

#[test]
fn a_newer_company_yaml_is_refused_and_nothing_written() {
    let e = env();
    let root = e.project("a", "schema_version: 9\nname: X\n");
    let msg = err(e.open(&root));
    assert!(msg.contains("newer than this app"), "{msg}");
    assert!(!e.places.base.dir().exists());
    assert_eq!(names(&root), vec!["company.yaml"]);
}

#[test]
fn a_failing_identity_tool_is_named_and_nothing_written_first_check_wins() {
    let e = env();
    // An empty folder: the identity tools run first, so the tool decides.
    let root = e.dir.path().join("empty");
    std::fs::create_dir(&root).unwrap();
    e.fake.fail_root(IdentityError::Tool {
        tool: "/usr/sbin/diskutil info -plist disk9".into(),
        output: "\"no VolumeUUID here\"".into(),
    });
    let msg = err(e.open(&root));
    assert!(msg.contains("/usr/sbin/diskutil"), "{msg}");
    assert!(msg.contains("no VolumeUUID here"), "{msg}");
    assert!(!e.places.base.dir().exists());

    let e = env();
    let root = e.project("a", COMPANY);
    e.fake.fail_mac_name(IdentityError::Tool {
        tool: "/usr/sbin/scutil --get ComputerName".into(),
        output: "nothing".into(),
    });
    let msg = err(e.open(&root));
    assert!(msg.contains("scutil"), "{msg}");
    assert_eq!(names(&root), vec!["company.yaml"]);
}

#[test]
fn a_network_share_is_not_supported_in_v1() {
    let e = env();
    let root = e.project("a", COMPANY);
    e.fake
        .fail_root(IdentityError::NetworkShare { path: root.clone() });
    let msg = err(e.open(&root));
    assert!(msg.contains("not supported in v1"), "{msg}");
    assert!(!e.places.base.dir().exists());
}

// ------------------------------------------------------------ copy and move

#[test]
fn a_copy_is_refused_naming_both_and_opens_as_new_after_its_id_is_removed() {
    let e = env();
    let a = e.project("a", COMPANY);
    let first = e.open(&a).unwrap().project_id;
    let b = e.dir.path().join("projects/b");
    copy_tree(&a, &b);
    let b = b.canonicalize().unwrap();
    let before = std::fs::read(b.join("company.yaml")).unwrap();

    let msg = err(e.open(&b));
    assert!(msg.contains(&a.display().to_string()), "{msg}");
    assert!(msg.contains(&b.display().to_string()), "{msg}");
    assert!(msg.contains("new project"), "{msg}");
    assert!(msg.contains("remove the `project_id` line"), "{msg}");
    assert_eq!(std::fs::read(b.join("company.yaml")).unwrap(), before);
    assert_eq!(e.runtime_folders(), vec![first.to_string()]);

    // Treat it as a new project: remove the id and open again.
    let text = String::from_utf8(before).unwrap();
    let removed: String = text
        .lines()
        .filter(|l| !l.starts_with("project_id"))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(b.join("company.yaml"), removed).unwrap();
    let o = e.open(&b).unwrap();
    assert_ne!(o.project_id, first);
    assert_eq!(id_of(&b), Some(o.project_id));
    assert_eq!(e.runtime_folders().len(), 2);
    let reader = o.store.reader().unwrap();
    assert!(
        reader.unfinished_runs().unwrap().is_empty(),
        "an empty store"
    );
}

#[test]
fn a_copy_whose_original_cannot_be_read_is_refused_not_moved() {
    let e = env();
    let a = e.project("a", COMPANY);
    let first = e.open(&a).unwrap().project_id;
    let b = e.dir.path().join("projects/b");
    copy_tree(&a, &b);
    let b = b.canonicalize().unwrap();
    // The original is still there, but its company.yaml cannot be read, so
    // whether it still holds the id is unknown: refused, never guessed.
    std::fs::write(a.join("company.yaml"), "schema_version: [\n").unwrap();
    let msg = err(e.open(&b));
    assert!(msg.contains(&a.display().to_string()), "{msg}");
    assert!(msg.contains("cannot tell"), "{msg}");
    let recs = records(&e.places.base);
    assert_eq!(recs.len(), 1);
    assert_eq!(
        recs[0].root.path, a,
        "the runtime state stays with the original"
    );
    assert_eq!(e.runtime_folders(), vec![first.to_string()]);
}

#[test]
fn a_renamed_project_folder_updates_its_recorded_path() {
    let e = env();
    let a = e.project("a", COMPANY);
    let id = e.open(&a).unwrap().project_id;
    let renamed = e.dir.path().join("projects/renamed");
    std::fs::rename(&a, &renamed).unwrap();
    let renamed = renamed.canonicalize().unwrap();
    let o = e.open(&renamed).unwrap();
    assert_eq!(o.project_id, id);
    drop(o);
    let recs = records(&e.places.base);
    assert_eq!(recs.len(), 1);
    assert_eq!(
        recs[0].root.path, renamed,
        "later refusals name where it is now"
    );
}

#[test]
fn an_indexed_root_whose_record_cannot_be_read_is_refused_not_minted_over() {
    let e = env();
    let a = e.project("a", COMPANY);
    let id = e.open(&a).unwrap().project_id;
    let record = e.layout(&id).root_record();
    std::fs::write(&record, "garbage\n").unwrap();
    // Another project's open rewrites the index; the unreadable record's
    // line must survive it.
    let other = e.project("other", COMPANY);
    drop(e.open(&other).unwrap());
    // The id line removed by hand.
    std::fs::write(a.join("company.yaml"), COMPANY).unwrap();
    let msg = err(e.open(&a));
    assert!(msg.contains(&id.to_string()), "{msg}");
    assert!(msg.contains(&record.display().to_string()), "{msg}");
    assert_eq!(e.runtime_folders().len(), 2, "no new runtime folder");
    assert_eq!(id_of(&a), None, "no id minted");
}

#[test]
fn a_moved_project_opens_records_its_new_root_and_audits_the_move() {
    let e = env();
    let a = e.project("a", COMPANY);
    let id = e.open(&a).unwrap().project_id;
    let b = e.dir.path().join("projects/b");
    copy_tree(&a, &b);
    std::fs::remove_dir_all(&a).unwrap();
    let b = b.canonicalize().unwrap();

    let o = e.open(&b).unwrap();
    assert_eq!(o.project_id, id);
    let recs = records(&e.places.base);
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].root.path, b);
    let moved = actions(&audit(&o), "project.moved");
    assert_eq!(moved.len(), 1);
    assert!(moved[0].contains(SYSTEM_RUNTIME), "{}", moved[0]);
    assert!(o.lines.iter().any(|l| l.contains("moved")), "{:?}", o.lines);
}

#[test]
fn an_unmounted_recorded_volume_opens_only_after_a_confirmed_move() {
    let e = env();
    let a = e.project("a", COMPANY);
    let id = e.open(&a).unwrap().project_id;
    let b = e.dir.path().join("projects/b");
    copy_tree(&a, &b);
    let b = b.canonicalize().unwrap();
    let volume = FakeProjectIdentity::identity_of(&a).unwrap().volume;
    e.fake.unmount(&volume);

    let msg = err(e.open(&b));
    assert!(msg.contains("not mounted"), "{msg}");
    assert!(msg.contains("--confirm-move"), "{msg}");
    // Without a terminal, refused saying one is needed.
    let msg = err(e.open_with(&b, true, false, &mut Answers::none()));
    assert!(msg.contains("need a terminal"), "{msg}");
    // No (the default) keeps it closed.
    let mut no = Answers::terminal(&[false]);
    let msg = err(e.open_with(&b, true, false, &mut no));
    assert!(msg.contains("stays closed"), "{msg}");
    assert_eq!(no.asked.len(), 1);
    assert_eq!(records(&e.places.base)[0].root.path, a, "not re-pointed");
    // Yes re-points only that root, audited by the Human.
    let mut yes = Answers::terminal(&[true]);
    let o = e.open_with(&b, true, false, &mut yes).unwrap();
    assert_eq!(o.project_id, id);
    let recs = records(&e.places.base);
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].root.path, b);
    let moved = actions(&audit(&o), "project.moved");
    assert_eq!(moved.len(), 1);
    assert!(moved[0].contains(HUMAN_OWNER), "{}", moved[0]);
}

// ------------------------------------------------------------ changed id

#[test]
fn a_changed_or_removed_id_is_refused_naming_both_ids_and_never_minted_over() {
    let e = env();
    let a = e.project("a", COMPANY);
    let id = e.open(&a).unwrap().project_id;
    let text = std::fs::read_to_string(a.join("company.yaml")).unwrap();
    let other = ProjectId::mint();
    std::fs::write(
        a.join("company.yaml"),
        text.replace(&id.to_string(), &other.to_string()),
    )
    .unwrap();
    for confirm in [false, true] {
        let mut yes = Answers::terminal(&[true]);
        let msg = err(e.open_with(&a, confirm, false, &mut yes));
        assert!(msg.contains(&id.to_string()), "{msg}");
        assert!(msg.contains(&other.to_string()), "{msg}");
        assert!(msg.contains("write the id back"), "{msg}");
        assert!(msg.contains("new project"), "{msg}");
        assert!(yes.asked.is_empty(), "--confirm-move is not asked");
    }
    // Removed: refused the same way, naming the indexed id and none.
    std::fs::write(a.join("company.yaml"), COMPANY).unwrap();
    let msg = err(e.open(&a));
    assert!(
        msg.contains(&id.to_string()) && msg.contains("none"),
        "{msg}"
    );
    assert_eq!(
        std::fs::read_to_string(a.join("company.yaml")).unwrap(),
        COMPANY
    );
    assert_eq!(e.runtime_folders(), vec![id.to_string()], "nothing minted");
    // The worded fix: write the indexed id back.
    std::fs::write(a.join("company.yaml"), text).unwrap();
    assert_eq!(e.open(&a).unwrap().project_id, id);
}

#[test]
fn a_changed_id_is_named_before_another_macs_marker() {
    let e = env();
    let a = e.project("a", COMPANY);
    let id = e.open(&a).unwrap().project_id;
    std::fs::write(a.join("company.yaml"), COMPANY).unwrap();
    write_marker(&a, &MacId::mint().to_string(), 1, "Other Mac");
    let msg = err(e.open(&a));
    assert!(msg.contains(&id.to_string()), "{msg}");
    assert!(!msg.contains("Other Mac"), "{msg}");
}

#[test]
fn a_crash_during_a_plain_mint_is_refused_as_a_removed_id_and_the_worded_fix_opens_it() {
    let e = env();
    let a = e.project("a", COMPANY);
    am_app::runtime::testing::fail_at(Some(am_app::runtime::testing::FailPoint::AfterRootRecord));
    let msg = err(e.open(&a));
    assert!(msg.contains("simulated crash"), "{msg}");
    am_app::runtime::testing::fail_at(None);
    let indexed = records(&e.places.base)[0].project_id;
    assert_eq!(id_of(&a), None, "the id was never written");

    let msg = err(e.open(&a));
    assert!(msg.contains(&indexed.to_string()), "{msg}");
    assert!(msg.contains("write the id back"), "{msg}");
    std::fs::write(
        a.join("company.yaml"),
        COMPANY.replacen(
            "schema_version: 1\n",
            &format!("schema_version: 1\nproject_id: {indexed}\n"),
            1,
        ),
    )
    .unwrap();
    assert_eq!(e.open(&a).unwrap().project_id, indexed);
}

#[test]
fn a_second_mac_or_restored_project_gets_its_folder_and_no_edit() {
    let e = env();
    let id = ProjectId::mint();
    let company =
        format!("schema_version: 1\nproject_id: {id}\nname: X\ntimezone: UTC\ncurrency: EUR\n");
    let a = e.project("a", &company);
    let o = e.open(&a).unwrap();
    assert_eq!(o.project_id, id);
    assert_eq!(
        std::fs::read_to_string(a.join("company.yaml")).unwrap(),
        company
    );
    let history = StoreConfigHistory::new(&o.store).unwrap();
    assert!(history.entries(None).unwrap().is_empty(), "no ConfigEdit");
    assert_eq!(records(&e.places.base)[0].root.path, a);
}

// ------------------------------------------------------------ legacy

/// A project as an older binary left it: `.agentic/` with a lock record, a
/// `.gitignore`, reference files and (optionally) a database with a row.
fn legacy_project(e: &Env, name: &str, company: &str, with_db: bool) -> PathBuf {
    let root = e.project(name, company);
    let old = legacy_dir(&root);
    std::fs::create_dir(&old).unwrap();
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(old.join(".gitignore"), "*\n").unwrap();
    std::fs::write(
        old.join("lock"),
        "pid: 1\nhost: old\ncommand: agentic\nstarted: x\n",
    )
    .unwrap();
    std::fs::create_dir_all(old.join("knowledge/seat-one")).unwrap();
    std::fs::write(old.join("knowledge/seat-one/guide.md"), "# Guide\n").unwrap();
    if with_db {
        let db = old.join("state.db");
        am_store::testing::database_at_version(&db, SCHEMA_VERSION - 1).unwrap();
        am_store::testing::insert_fixture_trigger(&db, "legacy-row").unwrap();
    }
    root
}

fn assert_owner_only(dir: &Path) {
    assert_eq!(mode(dir), 0o700, "{}", dir.display());
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            assert_owner_only(&e.path());
        } else {
            assert_eq!(mode(&e.path()), 0o600, "{}", e.path().display());
        }
    }
}

#[test]
fn a_legacy_folder_with_rows_moves_out_and_is_kept_and_reported() {
    let e = env();
    let root = legacy_project(&e, "a", COMMENTED, true);
    let o = e.open(&root).unwrap();
    let id = o.project_id;
    assert!(
        o.store
            .reader()
            .unwrap()
            .trigger("legacy-row")
            .unwrap()
            .is_some(),
        "the rows came over"
    );
    assert_eq!(id_of(&root), Some(id));
    assert_eq!(names(&root), vec![OPEN_MARKER, "company.yaml"]);
    let kept: Vec<String> = names(o.layout.dir())
        .into_iter()
        .filter(|n| n.starts_with(LEGACY_COPY_PREFIX))
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    let kept = o.layout.dir().join(&kept[0]);
    assert!(kept.join("state.db").is_file() && kept.join("lock").is_file());
    assert_owner_only(&kept);
    let line = o
        .lines
        .iter()
        .find(|l| l.contains(&kept.display().to_string()))
        .unwrap_or_else(|| panic!("{:?}", o.lines));
    assert!(line.contains("later deletions do not reach"), "{line}");
    assert!(line.contains("delete it by hand"), "{line}");
    assert!(
        o.lines.iter().any(|l| l.contains("migrated from version")),
        "{:?}",
        o.lines
    );
    assert_eq!(actions(&audit(&o), "project.legacy_moved").len(), 1);
    let j = am_store::legacy::read_journal(&o.layout).unwrap().unwrap();
    assert!(!j.unfinished());
}

#[test]
fn a_legacy_folder_with_only_reference_files_gets_a_fresh_store() {
    let e = env();
    let root = legacy_project(&e, "a", COMPANY, false);
    let o = e.open(&root).unwrap();
    assert!(
        o.lines.iter().any(|l| l.contains("created at version")),
        "{:?}",
        o.lines
    );
    assert!(o.layout.knowledge("seat-one").join("guide.md").is_file());
    assert!(!legacy_dir(&root).exists());
}

#[test]
fn a_held_legacy_lock_or_a_legacy_folder_beside_runtime_state_is_refused() {
    let e = env();
    let root = legacy_project(&e, "a", COMPANY, true);
    let held = std::fs::File::open(legacy_dir(&root).join("lock")).unwrap();
    held.try_lock().unwrap();
    let msg = err(e.open(&root));
    assert!(msg.contains("still open"), "{msg}");
    assert!(e.runtime_folders().is_empty(), "nothing written");
    drop(held);

    let e = env();
    let id = ProjectId::mint();
    let company =
        format!("schema_version: 1\nproject_id: {id}\nname: X\ntimezone: UTC\ncurrency: EUR\n");
    let root = legacy_project(&e, "b", &company, true);
    let layout = e.layout(&id);
    std::fs::create_dir_all(layout.dir()).unwrap();
    std::fs::set_permissions(layout.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
    am_store::testing::database_at_version(&layout.state_db(), SCHEMA_VERSION).unwrap();
    let msg = err(e.open(&root));
    assert!(
        msg.contains(&legacy_dir(&root).display().to_string()),
        "{msg}"
    );
    assert!(msg.contains(&layout.dir().display().to_string()), "{msg}");
    assert!(legacy_dir(&root).join("state.db").exists());
}

#[test]
fn a_legacy_folder_under_a_cloud_root_is_copied_verified_and_removed() {
    let e = env();
    let cloud = e.home().join("Library/CloudStorage/OneDrive-Personal");
    std::fs::create_dir_all(&cloud).unwrap();
    let root = cloud.join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("company.yaml"), COMPANY).unwrap();
    let old = legacy_dir(&root);
    std::fs::create_dir(&old).unwrap();
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(old.join("notes.txt"), "kept").unwrap();
    let o = e.open(&root).unwrap();
    assert!(
        am_store::legacy::testing::copied_kept_folder(),
        "the copy path ran"
    );
    assert!(!old.exists());
    assert!(
        o.lines.iter().any(|l| l.contains(LEGACY_COPY_PREFIX)),
        "{:?}",
        o.lines
    );
}

#[test]
fn a_crashed_legacy_move_resumes_from_its_journal_without_a_new_id() {
    use am_store::legacy::{FailPoint, testing};
    for point in [FailPoint::AfterCopy, FailPoint::AfterRename] {
        let e = env();
        let root = legacy_project(&e, "a", COMPANY, true);
        testing::fail_at(Some(point));
        let msg = err(e.open(&root));
        assert!(msg.contains("simulated crash"), "{point:?}: {msg}");
        testing::fail_at(None);
        assert_eq!(id_of(&root), None, "{point:?}");
        let minted = e.runtime_folders();
        assert_eq!(minted.len(), 1, "{point:?}");

        let o = e.open(&root).unwrap();
        assert_eq!(o.project_id.to_string(), minted[0], "{point:?}: no new id");
        assert_eq!(id_of(&root), Some(o.project_id));
        assert_eq!(e.runtime_folders(), minted);
        assert!(!legacy_dir(&root).exists());
        assert!(
            o.store
                .reader()
                .unwrap()
                .trigger("legacy-row")
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn a_failure_after_the_legacy_move_still_reports_and_audits_it_on_the_next_open() {
    use am_app::runtime::testing::{FailPoint, fail_at};
    let e = env();
    // An id is present, so nothing is minted: the journal must still stay
    // unfinished until the move is audited and reported.
    let id = ProjectId::mint();
    let company = format!(
        "schema_version: 1\nproject_id: {id}\nname: Example\ntimezone: UTC\ncurrency: EUR\n"
    );
    let root = legacy_project(&e, "a", &company, true);
    fail_at(Some(FailPoint::AfterLegacyMove));
    let msg = err(e.open(&root));
    assert!(msg.contains("simulated failure"), "{msg}");
    fail_at(None);
    assert!(!legacy_dir(&root).exists(), "the move itself happened");

    let o = e.open(&root).unwrap();
    assert_eq!(o.project_id, id);
    assert!(
        o.lines.iter().any(|l| l.contains(LEGACY_COPY_PREFIX)),
        "the kept folder is reported: {:?}",
        o.lines
    );
    assert_eq!(actions(&audit(&o), "project.legacy_moved").len(), 1);
}

#[test]
fn an_unverified_legacy_copy_is_refused_and_the_old_folder_is_untouched() {
    let e = env();
    let root = legacy_project(&e, "a", COMPANY, true);
    let before = std::fs::read(legacy_dir(&root).join("knowledge/seat-one/guide.md")).unwrap();
    am_store::legacy::testing::fail_at(Some(am_store::legacy::FailPoint::CorruptCopy));
    let msg = err(e.open(&root));
    assert!(msg.contains("did not match"), "{msg}");
    assert!(legacy_dir(&root).join("state.db").exists());
    assert_eq!(
        std::fs::read(legacy_dir(&root).join("knowledge/seat-one/guide.md")).unwrap(),
        before
    );
    assert_eq!(id_of(&root), None);
}

// ------------------------------------------------------------ database

#[test]
fn a_newer_runtime_or_legacy_database_is_refused_and_nothing_written() {
    let e = env();
    let a = e.project("a", COMPANY);
    let id = e.open(&a).unwrap().project_id;
    let db = e.layout(&id).state_db();
    std::fs::remove_file(&db).unwrap();
    am_store::testing::database_at_version(&db, SCHEMA_VERSION + 1).unwrap();
    let before = std::fs::read(&db).unwrap();
    let msg = err(e.open(&a));
    assert!(msg.contains("newer than this app knows"), "{msg}");
    assert_eq!(std::fs::read(&db).unwrap(), before);
    assert!(!open_marker(&a).exists());

    let e = env();
    let root = legacy_project(&e, "b", COMPANY, false);
    am_store::testing::database_at_version(&legacy_dir(&root).join("state.db"), SCHEMA_VERSION + 1)
        .unwrap();
    let msg = err(e.open(&root));
    assert!(msg.contains("newer than this app knows"), "{msg}");
    assert!(e.runtime_folders().is_empty());
    assert_eq!(id_of(&root), None);
}

// ------------------------------------------------------------ markers

fn write_marker(root: &Path, mac_id: &str, pid: u32, name: &str) {
    std::fs::write(
        open_marker(root),
        format!(
            "mac_name: {name}\nmac_id: {mac_id}\npid: {pid}\nopened_at: 2026-11-20T10:00:00.000Z\n"
        ),
    )
    .unwrap();
}

#[test]
fn another_macs_marker_refuses_until_a_take_over_on_a_terminal() {
    let e = env();
    let a = e.project("a", COMPANY);
    drop(e.open(&a).unwrap());
    write_marker(&a, &MacId::mint().to_string(), 4242, "Studio Mac");
    let msg = err(e.open(&a));
    assert!(msg.contains("Studio Mac"), "{msg}");
    assert!(msg.contains("--take-over"), "{msg}");
    let msg = err(e.open_with(&a, false, true, &mut Answers::none()));
    assert!(msg.contains("need a terminal"), "{msg}");
    let mut yes = Answers::terminal(&[true]);
    let o = e.open_with(&a, false, true, &mut yes).unwrap();
    assert!(yes.asked[0].contains("Studio Mac"), "{:?}", yes.asked);
    let taken = actions(&audit(&o), "project.taken_over");
    assert_eq!(taken.len(), 1);
    assert!(taken[0].contains(HUMAN_OWNER), "{}", taken[0]);
    let m = Marker::parse(&std::fs::read_to_string(open_marker(&a)).unwrap()).unwrap();
    assert_eq!(
        m.pid,
        std::process::id(),
        "the marker is now this process's"
    );
}

#[test]
fn this_macs_marker_is_refused_only_while_its_process_holds_the_project() {
    let e = env();
    let a = e.project("a", COMPANY);
    // Held: the project is open (its store lock taken) in this process.
    let held = e.open(&a).unwrap();
    let msg = err(e.open(&a));
    assert!(msg.contains("already open on this Mac"), "{msg}");
    assert!(
        msg.contains(&std::process::id().to_string()),
        "names the holder: {msg}"
    );
    drop(held);
    let mac = read_mac_id(&e.places.app_data)
        .unwrap()
        .unwrap()
        .to_string();
    // A live pid that does not hold the project (a pid reused after a crash
    // or a reboot): the marker is left over and replaced.
    let mut live = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    write_marker(&a, &mac, live.id(), "Test Mac");
    let o = e.open(&a).unwrap();
    let m = Marker::parse(&std::fs::read_to_string(open_marker(&a)).unwrap()).unwrap();
    assert_eq!(m.pid, std::process::id());
    drop(o);
    live.kill().unwrap();
    live.wait().unwrap();
    // The same pid, now gone: replaced.
    write_marker(&a, &mac, live.id(), "Test Mac");
    let o = e.open(&a).unwrap();
    let m = Marker::parse(&std::fs::read_to_string(open_marker(&a)).unwrap()).unwrap();
    assert_eq!(m.pid, std::process::id());
    drop(o);
}

#[test]
fn a_clean_close_leaves_a_marker_that_is_not_its_own() {
    let e = env();
    let a = e.project("a", COMPANY);
    let o = e.open(&a).unwrap();
    write_marker(&a, &MacId::mint().to_string(), 77, "Other Mac");
    drop(o);
    let m = Marker::parse(&std::fs::read_to_string(open_marker(&a)).unwrap()).unwrap();
    assert_eq!(m.mac_name, "Other Mac");
}

#[test]
fn the_decision_flags_are_refused_naming_the_ui_while_children_run_as_the_owner() {
    for (file, text) in [
        (
            "tools.yaml",
            "schema_version: 1\nmcp_servers:\n  - id: notes\n    transport: stdio\n    command: example-notes-server\n    version: 1.0.0\n    tools:\n      - name: list_notes\n        class: read\n",
        ),
        (
            "sources/site.yaml",
            "schema_version: 1\ntype: http-reader\nowner_seat: x\nrender: true\n",
        ),
    ] {
        let e = env();
        let a = e.project("a", COMPANY);
        drop(e.open(&a).unwrap());
        let path = a.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        write_marker(&a, &MacId::mint().to_string(), 4242, "Studio Mac");
        let mut yes = Answers::terminal(&[true]);
        let msg = err(e.open_with(&a, false, true, &mut yes));
        assert!(msg.contains("the UI"), "{file}: {msg}");
        assert!(yes.asked.is_empty(), "{file}: not asked");
        assert!(open_marker(&a).exists());
    }
}

// ------------------------------------------------------------ mac id

#[test]
fn this_macs_id_is_minted_once_reused_and_a_corrupt_one_is_refused() {
    let e = env();
    let a = e.project("a", COMPANY);
    assert_eq!(read_mac_id(&e.places.app_data).unwrap(), None);
    drop(e.open(&a).unwrap());
    let path = e.places.app_data.join("mac-id");
    let first = std::fs::read_to_string(&path).unwrap();
    assert_eq!(mode(&path), 0o600);
    drop(e.open(&a).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first, "reused");

    std::fs::write(&path, "not an id\n").unwrap();
    let msg = err(e.open(&a));
    assert!(msg.contains("does not hold an id"), "{msg}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "not an id\n",
        "not re-minted"
    );
}

// ------------------------------------------------------------ read-only

#[test]
fn the_read_only_resolve_refuses_in_words_and_writes_nothing() {
    let e = env();
    let none = e.dir.path().join("none");
    std::fs::create_dir(&none).unwrap();
    assert!(
        find_runtime(&e.places, &none)
            .unwrap_err()
            .0
            .contains("wizard")
    );
    let a = e.project("a", COMPANY);
    assert!(
        find_runtime(&e.places, &a)
            .unwrap_err()
            .0
            .contains("no project_id")
    );
    let id = ProjectId::mint();
    let b = e.project(
        "b",
        &format!("schema_version: 1\nproject_id: {id}\nname: X\n"),
    );
    assert!(
        find_runtime(&e.places, &b)
            .unwrap_err()
            .0
            .contains("no runtime folder")
    );
    std::fs::create_dir(legacy_dir(&b)).unwrap();
    assert!(
        find_runtime(&e.places, &b)
            .unwrap_err()
            .0
            .contains("old runtime folder")
    );
    assert!(!e.places.base.dir().exists());
    assert_eq!(e.fake.calls(), 0, "no identity tool");
}

#[test]
fn the_fake_is_a_project_identity() {
    let fake: &dyn ProjectIdentity = &FakeProjectIdentity::new("x");
    assert_eq!(fake.mac_name().unwrap(), "x");
}
