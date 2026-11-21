//! Story 1.9 acceptance tests (am-config side): outside edits wait for
//! Reload (R2, AD-16). A `ConfigSession` holds the accepted bytes of every
//! config file and the snapshot built only from them; a real `notify`
//! watcher on a temporary copy of `fixtures/projects/valid` announces each
//! outside change once as `ConfigEvent::ChangedOnDisk`; `reload` applies every
//! pending change as one new snapshot and records each file in History as an
//! `outside_edit` (faked here by am-testkit's `InMemoryConfigHistory`; the
//! real store runs in `crates/am-e2e/tests/config_reload.rs`). One test per
//! row of the plan's I/O & Edge-Case Matrix. Written before the session exists.
//!
//! The API these tests fix (plan Design Notes):
//! - `ConfigSession::open(root, history: Arc<dyn ConfigHistory>,
//!   on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync>)` starts the watcher;
//! - `session.snapshot()` (its `hash()` and entities), `session.apply(actor,
//!   edit)` (the writer, raising no event) and `session.reload(actor)` (one
//!   History entry per pending file, or a `ReloadError`);
//! - `ConfigEvent::ChangedOnDisk { file, before_sha256, after_sha256, diff,
//!   findings, identity_change }`;
//! - `ReloadError::{NothingPending, Invalid { findings }, IdentityChange}`.
//!
//! Timing (plan Design Notes): an event is awaited up to 10 s; 1.5 s of
//! silence proves that none came (the debounce is 300 ms).
#![forbid(unsafe_code)]

mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_config::{ConfigEdit, ConfigEvent, ConfigSession, EditOp, Finding, ReloadError, load};
use am_domain::{Actor, ConfigHistory, HistoryKind};
use am_testkit::InMemoryConfigHistory;
use common::{copy_of, write};
use serde_json::json;
use sha2::{Digest, Sha256};

const SEAT: &str = "seats/alpha.md";
const COMPANY: &str = "company.yaml";
/// How long an expected event may take.
const WAIT: Duration = Duration::from_secs(10);
/// How long without an event proves that none comes.
const QUIET: Duration = Duration::from_millis(1500);
/// A key-shaped canary (never a real key): `sk-ant-` and 8+ key characters.
const KEY: &str = "sk-ant-canary0canary0canary";
const ID_1: &str = "0192f0a0-0000-7000-8000-000000000001";
const ID_2: &str = "0192f0a0-0000-7000-8000-000000000002";

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

/// The text of `rel` with the first `from` replaced by `to`.
fn replaced(root: &Path, rel: &str, from: &str, to: &str) -> String {
    let text = String::from_utf8(read(root, rel)).unwrap();
    assert!(text.contains(from), "{rel} has no `{from}`");
    text.replacen(from, to, 1)
}

/// The hash `load` gives for the files on disk now (the project must be valid).
fn disk_hash(root: &Path) -> String {
    let loaded = load(root).unwrap();
    loaded
        .snapshot
        .unwrap_or_else(|| panic!("the project on disk is invalid: {:#?}", loaded.findings))
        .hash()
        .to_string()
}

/// Saves `text` at `rel` as an editor does, from another process: a temp
/// file beside it, then a rename over it.
fn save_from_another_process(root: &Path, rel: &str, text: &str) {
    let dest = root.join(rel);
    let name = dest.file_name().unwrap().to_string_lossy().into_owned();
    let tmp = dest.with_file_name(format!(".{name}.editor-swap"));
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"cat > "$1" && mv "$1" "$2""#)
        .arg("sh")
        .arg(&tmp)
        .arg(&dest)
        .stdin(Stdio::piped())
        .spawn()
        .expect("/bin/sh runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success(), "the outside save failed");
}

/// One `ChangedOnDisk` event's fields.
#[derive(Debug)]
struct Changed {
    file: String,
    before_sha256: Option<String>,
    after_sha256: Option<String>,
    diff: String,
    findings: Vec<Finding>,
    identity_change: bool,
}

fn changed(event: ConfigEvent) -> Changed {
    #[allow(unreachable_patterns)]
    match event {
        ConfigEvent::ChangedOnDisk {
            file,
            before_sha256,
            after_sha256,
            diff,
            findings,
            identity_change,
        } => Changed {
            file,
            before_sha256,
            after_sha256,
            diff,
            findings,
            identity_change,
        },
        other => panic!("expected ChangedOnDisk, got {other:?}"),
    }
}

/// A running session on a project folder, its fake History and its events.
struct Run {
    session: ConfigSession,
    history: Arc<InMemoryConfigHistory>,
    events: Receiver<ConfigEvent>,
}

impl Run {
    fn start(root: &Path) -> Run {
        let (tx, events) = channel();
        let tx = Mutex::new(tx);
        let on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync> = Arc::new(move |e| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(e);
            }
        });
        let history = Arc::new(InMemoryConfigHistory::default());
        let session = ConfigSession::open(root, history.clone(), on_event)
            .expect("a session opens on a valid project");
        Run {
            session,
            history,
            events,
        }
    }

    fn hash(&self) -> String {
        self.session.snapshot().hash().to_string()
    }

    /// The next event, waiting up to [`WAIT`].
    fn next(&self) -> Changed {
        match self.events.recv_timeout(WAIT) {
            Ok(e) => changed(e),
            Err(RecvTimeoutError::Timeout) => panic!("no event within {WAIT:?}"),
            Err(e) => panic!("the event channel closed: {e}"),
        }
    }

    /// No event for [`QUIET`].
    fn quiet(&self, why: &str) {
        match self.events.recv_timeout(QUIET) {
            Err(RecvTimeoutError::Timeout) => {}
            Ok(e) => panic!("{why}: an unexpected event {e:?}"),
            Err(e) => panic!("the event channel closed: {e}"),
        }
    }

    fn history_is_empty(&self) {
        assert!(self.history.entries(None).unwrap().is_empty());
        assert!(self.history.pending_intents().unwrap().is_empty());
    }
}

fn per_run_edit(value: serde_json::Value) -> ConfigEdit {
    ConfigEdit {
        file: SEAT.into(),
        ops: vec![EditOp::Set {
            path: vec!["budget".into(), "per_run".into()],
            value,
        }],
        body: None,
    }
}

// ------------------------------------------------------------- matrix rows

/// Outside edit: exactly one event with the diff; the snapshot hash stays.
#[test]
fn session_outside_edit_raises_exactly_one_event_and_keeps_the_snapshot() {
    let dir = copy_of("valid");
    let root = dir.path();
    let original = read(root, SEAT);
    let run = Run::start(root);
    let hash = run.hash();
    assert_eq!(hash, disk_hash(root), "the session starts from the disk");

    let new = replaced(root, SEAT, "per_run: 1.5", "per_run: 2");
    save_from_another_process(root, SEAT, &new);

    let e = run.next();
    assert_eq!(e.file, SEAT);
    assert_eq!(e.before_sha256.as_deref(), Some(sha(&original).as_str()));
    assert_eq!(
        e.after_sha256.as_deref(),
        Some(sha(new.as_bytes()).as_str())
    );
    assert!(e.findings.is_empty(), "{:#?}", e.findings);
    assert!(!e.identity_change);
    assert!(
        e.diff
            .starts_with("--- a/seats/alpha.md\n+++ b/seats/alpha.md\n@@ "),
        "{}",
        e.diff
    );
    assert!(e.diff.contains("\n-  per_run: 1.5\n"), "{}", e.diff);
    assert!(e.diff.contains("\n+  per_run: 2\n"), "{}", e.diff);
    run.quiet("one outside save is announced once");

    assert_eq!(run.hash(), hash, "nothing applies before Reload");
    run.history_is_empty();
}

/// Reload: the new hash is live, one `outside_edit` entry with the diff and
/// the Actor; the file on disk is not rewritten.
#[test]
fn session_reload_applies_the_pending_edit_and_records_one_outside_edit() {
    let dir = copy_of("valid");
    let root = dir.path();
    let original = read(root, SEAT);
    let run = Run::start(root);
    let old = run.hash();

    let new = replaced(root, SEAT, "per_run: 1.5", "per_run: 2");
    save_from_another_process(root, SEAT, &new);
    let e = run.next();

    let entries = run
        .session
        .reload(reviewer())
        .expect("a valid edit reloads");
    assert_eq!(entries.len(), 1, "{entries:#?}");
    let entry = &entries[0];
    assert_eq!(entry.kind, HistoryKind::OutsideEdit);
    assert_eq!(entry.kind.as_str(), "outside_edit");
    assert_eq!(entry.file, SEAT);
    assert_eq!(entry.ref_entry, None);
    assert_eq!(entry.actor, reviewer());
    assert_eq!(entry.who(), "You (Reviewer)");
    assert_eq!(entry.before, original);
    assert_eq!(entry.after, new.as_bytes());
    assert_eq!(entry.before_sha256, sha(&original));
    assert_eq!(entry.after_sha256, sha(new.as_bytes()));
    assert_eq!(entry.diff, e.diff, "History keeps the announced diff");
    assert_eq!(run.history.entries(None).unwrap(), entries);
    assert!(run.history.pending_intents().unwrap().is_empty());

    let live = run.hash();
    assert_ne!(live, old);
    assert_eq!(live, disk_hash(root), "the new hash is live");
    let budget = run.session.snapshot().seats()["alpha"].budget.clone();
    let on_disk = load(root).unwrap().snapshot.unwrap().seats()["alpha"]
        .budget
        .clone();
    assert_eq!(budget, on_disk, "the snapshot holds the new value");
    assert_eq!(read(root, SEAT), new.as_bytes(), "Reload never rewrites");

    let again = run.session.reload(reviewer()).unwrap_err();
    assert!(
        matches!(again, ReloadError::NothingPending { .. }),
        "{again:?}"
    );
    assert_eq!(run.history.entries(None).unwrap().len(), 1);
    run.quiet("Reload raises no event");
}

/// Own write: no event; the snapshot moves to the new bytes. A later outside
/// edit is compared with those bytes.
#[test]
fn session_own_write_raises_no_event_and_moves_the_snapshot() {
    let dir = copy_of("valid");
    let root = dir.path();
    let run = Run::start(root);
    let old = run.hash();

    let entry = run
        .session
        .apply(reviewer(), per_run_edit(json!(2)))
        .expect("the edit applies");
    assert_eq!(entry.kind, HistoryKind::Edit);
    run.quiet("the app's own write raises no event");
    let live = run.hash();
    assert_ne!(live, old);
    assert_eq!(live, disk_hash(root), "the snapshot moves to the new bytes");
    assert_eq!(read(root, SEAT), entry.after);

    // The accepted bytes are the written ones.
    let outside = replaced(root, SEAT, "per_run: 2", "per_run: 3");
    save_from_another_process(root, SEAT, &outside);
    let e = run.next();
    assert_eq!(e.file, SEAT);
    assert_eq!(
        e.before_sha256.as_deref(),
        Some(entry.after_sha256.as_str())
    );
    assert_eq!(run.hash(), live);
}

/// Burst: three saves of the same content within the debounce window give
/// one event.
#[test]
fn session_burst_of_saves_raises_one_event() {
    let dir = copy_of("valid");
    let root = dir.path();
    let run = Run::start(root);
    let hash = run.hash();

    let new = replaced(root, SEAT, "per_run: 1.5", "per_run: 2");
    for _ in 0..3 {
        write(root, SEAT, &new);
        std::thread::sleep(Duration::from_millis(50));
    }
    let e = run.next();
    assert_eq!(e.file, SEAT);
    assert_eq!(
        e.after_sha256.as_deref(),
        Some(sha(new.as_bytes()).as_str())
    );
    run.quiet("a burst of the same content is announced once");
    assert_eq!(run.hash(), hash);
}

/// Second change before Reload: a second event for the new content; Reload
/// applies the latest bytes in one entry.
#[test]
fn session_second_change_before_reload_raises_a_second_event_and_reload_takes_the_latest() {
    let dir = copy_of("valid");
    let root = dir.path();
    let original = read(root, SEAT);
    let run = Run::start(root);
    let hash = run.hash();

    let first = replaced(root, SEAT, "per_run: 1.5", "per_run: 2");
    save_from_another_process(root, SEAT, &first);
    let e1 = run.next();
    assert_eq!(
        e1.after_sha256.as_deref(),
        Some(sha(first.as_bytes()).as_str())
    );

    let second = replaced(root, SEAT, "per_run: 2", "per_run: 3");
    save_from_another_process(root, SEAT, &second);
    let e2 = run.next();
    assert_eq!(e2.file, SEAT);
    assert_eq!(
        e2.before_sha256.as_deref(),
        Some(sha(&original).as_str()),
        "compared with the accepted bytes"
    );
    assert_eq!(
        e2.after_sha256.as_deref(),
        Some(sha(second.as_bytes()).as_str())
    );
    assert!(e2.diff.contains("\n+  per_run: 3\n"), "{}", e2.diff);
    run.quiet("each distinct content is announced once");
    assert_eq!(run.hash(), hash);

    let entries = run.session.reload(reviewer()).unwrap();
    assert_eq!(entries.len(), 1, "one entry per file: {entries:#?}");
    assert_eq!(entries[0].before, original);
    assert_eq!(entries[0].after, second.as_bytes());
    assert_eq!(run.hash(), disk_hash(root));
}

/// Invalid edit: the event carries findings with file, line and fix; the
/// snapshot stays; Reload is refused and History gets nothing.
#[test]
fn session_invalid_outside_edit_is_reported_with_findings_and_never_applied() {
    let dir = copy_of("valid");
    let root = dir.path();
    let run = Run::start(root);
    let hash = run.hash();

    let new = replaced(root, SEAT, "team: core", "team: no-such-team");
    save_from_another_process(root, SEAT, &new);
    let e = run.next();
    assert_eq!(e.file, SEAT);
    let team = e
        .findings
        .iter()
        .find(|f| f.file == SEAT && f.line == 5 && f.message.contains("no-such-team"))
        .unwrap_or_else(|| panic!("no unknown-team finding at {SEAT}:5: {:#?}", e.findings));
    assert!(!team.fix.is_empty(), "{team:?}");
    assert!(e.diff.contains("\n+team: no-such-team\n"), "{}", e.diff);
    assert_eq!(run.hash(), hash);

    let err = run.session.reload(reviewer()).unwrap_err();
    match &err {
        ReloadError::Invalid { findings } => assert!(
            findings
                .iter()
                .any(|f| f.file == SEAT && f.line == 5 && !f.fix.is_empty()),
            "{findings:#?}"
        ),
        other => panic!("expected Invalid, got {other:?}"),
    }
    assert!(err.to_string().contains(SEAT), "{err}");
    assert_eq!(run.hash(), hash, "an invalid change never applies");
    run.history_is_empty();
    assert_eq!(read(root, SEAT), new.as_bytes(), "Reload never rewrites");
}

/// Secret pasted: the diff is withheld with a worded note naming the line;
/// the change is invalid; Reload is refused and History gets nothing.
#[test]
fn session_secret_in_an_outside_edit_withholds_the_diff_and_is_refused() {
    let dir = copy_of("valid");
    let root = dir.path();
    let run = Run::start(root);
    let hash = run.hash();

    let text = String::from_utf8(read(root, SEAT)).unwrap();
    let line = text.lines().count() + 1;
    let new = format!("{text}Use {KEY} for the feed.\n");
    save_from_another_process(root, SEAT, &new);
    let e = run.next();
    assert_eq!(e.file, SEAT);
    assert!(
        !format!("{e:?}").contains("sk-ant-canary"),
        "the key reached the event: {e:?}"
    );
    assert!(
        !e.diff.starts_with("--- "),
        "the diff is withheld: {}",
        e.diff
    );
    assert!(e.diff.contains("secret"), "a worded note: {}", e.diff);
    assert!(
        e.diff.contains(&format!("line {line}")),
        "the note names line {line}: {}",
        e.diff
    );
    assert!(
        e.findings
            .iter()
            .any(|f| f.file == SEAT && f.line as usize == line),
        "invalid: {:#?}",
        e.findings
    );
    assert_eq!(run.hash(), hash);

    let err = run.session.reload(reviewer()).unwrap_err();
    assert!(matches!(err, ReloadError::Invalid { .. }), "{err:?}");
    assert!(!format!("{err:?}{err}").contains("sk-ant-canary"), "{err}");
    assert_eq!(run.hash(), hash);
    run.history_is_empty();
}

/// Created and deleted files: an event each (before or after empty); Reload
/// applies both.
#[test]
fn session_created_and_deleted_files_raise_an_event_each_and_reload_applies_both() {
    let dir = copy_of("valid");
    let root = dir.path();
    // A third seat nothing refers to, so removing it keeps the project valid.
    let gamma = replaced(root, SEAT, "name: Alpha", "name: Gamma");
    write(root, "seats/gamma.md", &gamma);
    let run = Run::start(root);
    let hash = run.hash();
    assert!(run.session.snapshot().seats().contains_key("gamma"));

    let workflow = String::from_utf8(read(root, "workflows/digest.yaml")).unwrap();
    write(root, "workflows/x.yaml", &workflow);
    std::fs::remove_file(root.join("seats/gamma.md")).unwrap();

    let mut events = [run.next(), run.next()];
    events.sort_by(|a, b| a.file.cmp(&b.file));
    let (deleted, created) = (&events[0], &events[1]);
    assert_eq!(created.file, "workflows/x.yaml");
    assert_eq!(created.before_sha256, None);
    assert_eq!(
        created.after_sha256.as_deref(),
        Some(sha(workflow.as_bytes()).as_str())
    );
    assert!(
        created.diff.contains("\n+schema_version: 1\n"),
        "{}",
        created.diff
    );
    assert!(created.findings.is_empty(), "{:#?}", created.findings);
    assert_eq!(deleted.file, "seats/gamma.md");
    assert_eq!(
        deleted.before_sha256.as_deref(),
        Some(sha(gamma.as_bytes()).as_str())
    );
    assert_eq!(deleted.after_sha256, None);
    assert!(
        deleted.diff.contains("\n-name: Gamma\n"),
        "{}",
        deleted.diff
    );
    run.quiet("one event per file");
    assert_eq!(run.hash(), hash);

    let mut entries = run.session.reload(reviewer()).unwrap();
    entries.sort_by(|a, b| a.file.cmp(&b.file));
    assert_eq!(entries.len(), 2, "{entries:#?}");
    assert!(entries.iter().all(|e| e.kind == HistoryKind::OutsideEdit));
    assert_eq!(entries[0].file, "seats/gamma.md");
    assert_eq!(entries[0].before, gamma.as_bytes());
    assert!(entries[0].after.is_empty());
    assert_eq!(entries[1].file, "workflows/x.yaml");
    assert!(entries[1].before.is_empty());
    assert_eq!(entries[1].after, workflow.as_bytes());

    assert_eq!(run.hash(), disk_hash(root));
    let snap = run.session.snapshot();
    assert!(snap.workflows().contains_key("x"));
    assert!(!snap.seats().contains_key("gamma"));
    assert!(
        !root.join("seats/gamma.md").exists(),
        "Reload creates nothing"
    );
}

/// Identity change: a changed or removed `project_id` is announced as one
/// and Reload refuses it in words.
#[test]
fn session_identity_change_is_flagged_and_reload_refuses_it() {
    let dir = copy_of("valid");
    let root = dir.path();
    let with_id = replaced(
        root,
        COMPANY,
        "schema_version: 1\n",
        &format!("schema_version: 1\nproject_id: {ID_1}\n"),
    );
    let without_id = String::from_utf8(read(root, COMPANY)).unwrap();
    write(root, COMPANY, &with_id);
    let run = Run::start(root);
    let hash = run.hash();

    // Edited.
    save_from_another_process(root, COMPANY, &with_id.replace(ID_1, ID_2));
    let e = run.next();
    assert_eq!(e.file, COMPANY);
    assert!(e.identity_change, "{e:?}");
    let err = run.session.reload(reviewer()).unwrap_err();
    assert!(matches!(err, ReloadError::IdentityChange { .. }), "{err:?}");
    assert!(err.to_string().contains("project_id"), "in words: {err}");
    assert_eq!(run.hash(), hash);
    run.history_is_empty();

    // Removed.
    save_from_another_process(root, COMPANY, &without_id);
    let e = run.next();
    assert_eq!(e.file, COMPANY);
    assert!(e.identity_change, "{e:?}");
    assert_eq!(
        e.before_sha256.as_deref(),
        Some(sha(with_id.as_bytes()).as_str())
    );
    let err = run.session.reload(reviewer()).unwrap_err();
    assert!(matches!(err, ReloadError::IdentityChange { .. }), "{err:?}");
    assert!(err.to_string().contains("project_id"), "in words: {err}");
    assert_eq!(run.hash(), hash);
    run.history_is_empty();
}

/// Not config: temp files, the open marker, notes and `.git/` raise nothing.
#[test]
fn session_files_that_are_not_config_raise_no_event() {
    let dir = copy_of("valid");
    let root = dir.path();
    let run = Run::start(root);
    let hash = run.hash();

    write(
        root,
        "seats/.alpha.md.am-tmp",
        "---\nschema_version: 1\n---\n",
    );
    write(root, am_domain::runtime::OPEN_MARKER, "{}\n");
    write(root, "notes.txt", "a note\n");
    write(root, "seats/notes.txt", "a note beside the seats\n");
    write(root, ".git/HEAD", "ref: refs/heads/main\n");
    write(root, ".git/config", "[core]\n");
    run.quiet("files that are not config");
    assert_eq!(run.hash(), hash);
    let err = run.session.reload(reviewer()).unwrap_err();
    assert!(matches!(err, ReloadError::NothingPending { .. }), "{err:?}");
}

/// Nothing pending: Reload answers `NothingPending` and records nothing.
#[test]
fn session_reload_with_nothing_pending_records_nothing() {
    let dir = copy_of("valid");
    let root = dir.path();
    let run = Run::start(root);
    let hash = run.hash();
    let err = run.session.reload(reviewer()).unwrap_err();
    assert!(matches!(err, ReloadError::NothingPending { .. }), "{err:?}");
    assert_eq!(run.hash(), hash);
    run.history_is_empty();
}

// ------------------------------------------------------ review fixes (1.9)

/// A session's open, for tests that expect it to fail.
fn open_err(root: &Path) -> am_config::SessionError {
    let on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync> = Arc::new(|_| {});
    match ConfigSession::open(root, Arc::new(InMemoryConfigHistory::default()), on_event) {
        Ok(_) => panic!("the session opened"),
        Err(e) => e,
    }
}

/// Own-write fingerprints cover only writes in flight: after the app writes
/// A to B and undoes back to A, an editor saving B again is announced, and
/// Reload applies it.
#[test]
fn session_outside_save_matching_an_earlier_app_write_is_announced() {
    let dir = copy_of("valid");
    let root = dir.path();
    let run = Run::start(root);
    let wrote = run
        .session
        .apply(reviewer(), per_run_edit(json!(2)))
        .expect("the edit applies");
    run.session
        .undo(reviewer(), &wrote.id)
        .expect("the undo applies");
    run.quiet("own writes raise no event");
    let hash = run.hash();

    // An editor still holding B saves it.
    let b = String::from_utf8(wrote.after.clone()).unwrap();
    save_from_another_process(root, SEAT, &b);
    let e = run.next();
    assert_eq!(e.file, SEAT);
    assert_eq!(e.after_sha256.as_deref(), Some(wrote.after_sha256.as_str()));
    run.quiet("announced once");
    assert_eq!(run.hash(), hash);

    let entries = run.session.reload(reviewer()).expect("the save reloads");
    assert_eq!(entries.len(), 1, "{entries:#?}");
    assert_eq!(entries[0].kind, HistoryKind::OutsideEdit);
    assert_eq!(entries[0].after, wrote.after);
    assert_eq!(run.hash(), disk_hash(root));
}

/// A link in a skill's references/ is refused by the session's open as by
/// `load`.
#[test]
fn session_open_refuses_a_link_in_skill_references_as_load_does() {
    let dir = copy_of("valid");
    let root = dir.path();
    let refs = root.join("skills/brief/references");
    std::os::unix::fs::symlink(refs.join("style.md"), refs.join("link.md")).unwrap();
    let link = |found: &[Finding]| {
        found
            .iter()
            .find(|f| {
                f.file == "skills/brief/references/link.md" && f.message.contains("is a link")
            })
            .cloned()
    };
    let by_load = link(&load(root).unwrap().findings).expect("load reports the link");
    match open_err(root) {
        am_config::SessionError::Invalid { findings } => {
            assert_eq!(link(&findings), Some(by_load), "{findings:#?}")
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

/// Undo of a file created outside would delete it: refused in words, and
/// the file stays.
#[test]
fn session_undo_of_an_outside_create_is_refused_in_words() {
    let dir = copy_of("valid");
    let root = dir.path();
    let run = Run::start(root);
    let workflow = String::from_utf8(read(root, "workflows/digest.yaml")).unwrap();
    write(root, "workflows/x.yaml", &workflow);
    let e = run.next();
    assert_eq!(e.file, "workflows/x.yaml");
    let entries = run.session.reload(reviewer()).unwrap();
    assert_eq!(entries.len(), 1);
    let hash = run.hash();

    let err = run
        .session
        .undo(reviewer(), &entries[0].id)
        .expect_err("undo of an outside create is refused");
    let words = err.to_string();
    assert!(words.contains("by hand"), "{words}");
    assert!(words.contains("workflows/x.yaml"), "{words}");
    assert_eq!(read(root, "workflows/x.yaml"), workflow.as_bytes());
    assert_eq!(run.hash(), hash);
    assert_eq!(run.history.entries(None).unwrap().len(), 1);
    run.quiet("a refused undo writes nothing");
}

/// A pending change reverted on disk to the accepted bytes is withdrawn
/// with one event; nothing is left to reload.
#[test]
fn session_reverted_outside_edit_is_withdrawn() {
    let dir = copy_of("valid");
    let root = dir.path();
    let original = String::from_utf8(read(root, SEAT)).unwrap();
    let run = Run::start(root);
    let hash = run.hash();

    save_from_another_process(
        root,
        SEAT,
        &original.replacen("per_run: 1.5", "per_run: 2", 1),
    );
    assert_eq!(run.next().file, SEAT);
    save_from_another_process(root, SEAT, &original);
    match run.events.recv_timeout(WAIT) {
        Ok(ConfigEvent::Withdrawn { file }) => assert_eq!(file, SEAT),
        other => panic!("expected Withdrawn, got {other:?}"),
    }
    run.quiet("withdrawn once");
    let err = run.session.reload(reviewer()).unwrap_err();
    assert!(matches!(err, ReloadError::NothingPending { .. }), "{err:?}");
    assert_eq!(run.hash(), hash);
    run.history_is_empty();
}

/// `open_in` loads with the caller's options, as `load_in` does: a template
/// folder refuses a `project_id` that `open`'s defaults accept.
#[test]
fn session_open_in_loads_with_the_given_options() {
    let dir = copy_of("valid");
    let root = dir.path();
    let with_id = replaced(
        root,
        COMPANY,
        "schema_version: 1\n",
        &format!("schema_version: 1\nproject_id: {ID_1}\n"),
    );
    write(root, COMPANY, &with_id);
    let opts = am_config::LoadOptions {
        knowledge_root: None,
        template: true,
    };
    let expected: Vec<Finding> = am_config::load_in(root, opts.clone())
        .unwrap()
        .findings
        .into_iter()
        .filter(|f| f.severity == am_config::Severity::Error)
        .collect();
    assert!(
        expected.iter().any(|f| f.file == COMPANY),
        "load_in refuses the project_id: {expected:#?}"
    );
    let on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync> = Arc::new(|_| {});
    let history = Arc::new(InMemoryConfigHistory::default());
    match ConfigSession::open_in(root, opts, history, on_event) {
        Ok(_) => panic!("open_in ignored the template option"),
        Err(am_config::SessionError::Invalid { findings }) => assert_eq!(findings, expected),
        Err(other) => panic!("expected Invalid, got {other:?}"),
    }
    // `open` keeps the project defaults.
    let run = Run::start(root);
    assert_eq!(run.hash(), disk_hash(root));
}
