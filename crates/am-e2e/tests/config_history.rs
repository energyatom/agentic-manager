//! Story 1.8 acceptance test (R3, AD-16, AD-24, AD-25): a seat file edited
//! through the command path on a copy of `fixtures/projects/valid` with a
//! real project store. The file is replaced atomically with canonical key
//! order and an unchanged Markdown body; History shows the diff with the
//! Actor and hat in words; Undo writes the previous bytes back as a new
//! entry; Restore of an older entry works; each step has its audit entry,
//! the log verifies and no file content reaches `args`. Then the refusals
//! and the crash rows of the plan's matrix. Written before the writer exists.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use am_config::{ConfigEdit, ConfigWriter, EditOp, FailPoint, WriteError};
use am_domain::{Actor, ConfigHistory, HistoryKind};
use am_store::{Store, StoreConfigHistory, open_project, verify_jsonl};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SEAT: &str = "seats/alpha.md";
/// Stands for private file content: it must never reach the audit log.
const CANARY: &str = "am-config-canary-9e02 keep these instructions private";

fn reviewer() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Reviewer".into(),
    }
}

fn system() -> Actor {
    Actor::System {
        component: "config-writer".into(),
    }
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// A copy of the valid fixture whose seat body holds the canary.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/projects/valid");
    copy_dir(&fixture, dir.path());
    let seat = dir.path().join(SEAT);
    let text = std::fs::read_to_string(&seat).unwrap();
    std::fs::write(&seat, format!("{text}{CANARY}\n")).unwrap();
    dir
}

struct Open {
    store: Store,
    history: Arc<StoreConfigHistory>,
    writer: ConfigWriter,
}

fn open(root: &Path) -> Open {
    let (store, _) = open_project(root).unwrap();
    let history = Arc::new(StoreConfigHistory::new(&store).unwrap());
    let writer = ConfigWriter::open(root, history.clone()).unwrap();
    Open {
        store,
        history,
        writer,
    }
}

fn read(root: &Path) -> Vec<u8> {
    std::fs::read(root.join(SEAT)).unwrap()
}

fn set_per_run(v: Value) -> ConfigEdit {
    ConfigEdit {
        file: SEAT.into(),
        ops: vec![EditOp::Set {
            path: vec!["budget".into(), "per_run".into()],
            value: v,
        }],
        body: None,
    }
}

fn body_of(bytes: &[u8]) -> String {
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    let rest = text.strip_prefix("---\n").unwrap();
    rest.split_once("\n---\n").unwrap().1.to_string()
}

/// The exported audit log (checked), and its `config.*` entries.
fn audit(store: &Store) -> (String, Vec<Value>) {
    let mut out = Vec::new();
    store
        .reader()
        .unwrap()
        .export_audit_jsonl(&mut out)
        .unwrap();
    verify_jsonl(out.as_slice()).expect("the audit chain verifies");
    let text = String::from_utf8(out).unwrap();
    let config = text
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|e| e["action"].as_str().unwrap().starts_with("config."))
        .collect();
    (text, config)
}

#[test]
fn config_history_edit_undo_restore_through_the_store() {
    let dir = project();
    let root = dir.path();
    let original = read(root);
    let o = open(root);

    // Edit.
    let e1 = o.writer.apply(reviewer(), set_per_run(json!(2))).unwrap();
    let after1 = read(root);
    assert_ne!(after1, original);
    assert_eq!(
        body_of(&after1),
        body_of(&original),
        "the body is kept byte for byte"
    );
    assert!(String::from_utf8_lossy(&after1).contains("budget:\n  per_run: 2\n"));
    assert_eq!(e1.who(), "You (Reviewer)");
    assert!(e1.diff.contains("+  per_run: 2"), "{}", e1.diff);
    assert!(!root.join("seats/.alpha.md.am-tmp").exists());

    // A second edit, so Restore has an older entry to go back to.
    let e2 = o.writer.apply(reviewer(), set_per_run(json!(3))).unwrap();

    // Undo the latest.
    let e3 = o.writer.undo(reviewer(), &e2.id).unwrap();
    assert_eq!(read(root), after1, "Undo writes the previous bytes back");
    assert_eq!(e3.kind, HistoryKind::Undo);
    assert_eq!(e3.ref_entry.as_deref(), Some(e2.id.as_str()));

    // Undo of an entry the file has moved on from is refused.
    let err = o.writer.undo(reviewer(), &e2.id).unwrap_err();
    assert!(
        err.to_string().contains("changed since this entry"),
        "{err}"
    );

    // Restore the first entry's state after a third edit.
    let e4 = o.writer.apply(reviewer(), set_per_run(json!(4))).unwrap();
    let e5 = o.writer.restore(reviewer(), &e1.id).unwrap();
    assert_eq!(read(root), e1.after);
    assert_eq!(e5.kind, HistoryKind::Restore);
    assert_eq!(e5.before, e4.after);

    // History in the store, in order, with the Actor in words.
    let entries = o.history.entries(Some(SEAT.into())).unwrap();
    let kinds: Vec<HistoryKind> = entries.iter().map(|e| e.kind).collect();
    use HistoryKind::*;
    assert_eq!(kinds, [Edit, Edit, Undo, Edit, Restore]);
    assert!(entries.iter().all(|e| e.who() == "You (Reviewer)"));
    assert!(o.history.pending_intents().unwrap().is_empty());

    // One completed audit entry per step, each naming its History entry;
    // intents are audited when opened; no content anywhere in the log.
    let (text, config) = audit(&o.store);
    assert!(
        !text.contains("am-config-canary"),
        "file content reached the audit log"
    );
    let done: Vec<(&str, &str)> = config
        .iter()
        .filter(|e| e["action"] != "config.intent_opened")
        .map(|e| (e["action"].as_str().unwrap(), e["target"].as_str().unwrap()))
        .collect();
    assert_eq!(
        done,
        [
            ("config.edited", e1.id.as_str()),
            ("config.edited", e2.id.as_str()),
            ("config.undone", e3.id.as_str()),
            ("config.edited", e4.id.as_str()),
            ("config.restored", e5.id.as_str()),
        ]
    );
    let opened = config
        .iter()
        .filter(|e| e["action"] == "config.intent_opened")
        .count();
    assert_eq!(opened, 5);
    for e in &config {
        let args = e["args"].as_object().unwrap();
        for (k, v) in args {
            assert!(
                [
                    "after_sha256",
                    "before_sha256",
                    "entry_id",
                    "file",
                    "kind",
                    "ref_entry"
                ]
                .contains(&k.as_str()),
                "{k} in {e}"
            );
            if k.ends_with("sha256") {
                assert_eq!(v.as_str().unwrap().len(), 64);
            }
        }
        assert_eq!(e["args"]["file"], SEAT);
        assert_eq!(
            e["actor"],
            json!({"kind": "human", "person": "owner", "hat": "Reviewer"})
        );
    }
    assert_eq!(config[1]["args"]["after_sha256"], sha(&after1).as_str());
}

#[test]
fn config_history_refusals_write_nothing() {
    let dir = project();
    let root = dir.path();
    let original = read(root);
    let o = open(root);
    let (_, before) = audit(&o.store);

    // An invalid result: unknown team.
    let err = o
        .writer
        .apply(
            reviewer(),
            ConfigEdit {
                file: SEAT.into(),
                ops: vec![EditOp::Set {
                    path: vec!["team".into()],
                    value: json!("no-such-team"),
                }],
                body: None,
            },
        )
        .unwrap_err();
    assert!(matches!(err, WriteError::Invalid { .. }), "{err:?}");
    assert!(err.to_string().contains(&format!("{SEAT}:")), "{err}");
    assert!(err.to_string().contains("fix:"), "{err}");

    // Bad paths.
    for file in ["../x.yaml", "/etc/x", "seats/missing.md"] {
        let mut edit = set_per_run(json!(2));
        edit.file = file.into();
        let err = o.writer.apply(reviewer(), edit).unwrap_err();
        assert!(matches!(err, WriteError::BadPath { .. }), "{file}: {err:?}");
    }

    assert_eq!(read(root), original);
    assert!(o.history.entries(None).unwrap().is_empty());
    assert!(o.history.pending_intents().unwrap().is_empty());
    let (_, after) = audit(&o.store);
    assert_eq!(after.len(), before.len(), "a refusal opens no intent");
}

#[test]
fn config_history_crash_rows_are_reconciled_at_the_next_start() {
    let dir = project();
    let root = dir.path();
    let original = read(root);

    // Crash after the rename: the intent is committed, the file renamed,
    // History not committed.
    {
        let o = open(root);
        o.writer.fail_at(Some(FailPoint::AfterRename));
        let err = o
            .writer
            .apply(reviewer(), set_per_run(json!(2)))
            .unwrap_err();
        assert!(matches!(err, WriteError::Interrupted(_)), "{err:?}");
    }
    let renamed = read(root);
    assert_ne!(renamed, original);

    // Crash before the rename of an edit of another file: it keeps its
    // bytes. (A later write of the same file would settle the first intent
    // itself; see the pending-intent test below.)
    const OTHER: &str = "seats/beta.md";
    let other = std::fs::read(root.join(OTHER)).unwrap();
    {
        let o = open(root);
        o.writer.fail_at(Some(FailPoint::BeforeRename));
        let mut edit = set_per_run(json!(3));
        edit.file = OTHER.into();
        o.writer.apply(reviewer(), edit).unwrap_err();
    }
    assert_eq!(read(root), renamed);
    assert_eq!(std::fs::read(root.join(OTHER)).unwrap(), other);

    // The next start: no temp file is left, the renamed edit is completed
    // (audited as reconciled, by the system) and the other intent dropped.
    let o = open(root);
    assert!(!root.join("seats/.beta.md.am-tmp").exists());
    assert_eq!(o.history.pending_intents().unwrap().len(), 2);
    let report = o.writer.reconcile(system()).unwrap();
    assert_eq!(report.completed.len(), 1);
    assert_eq!(report.dropped.len(), 1);
    assert_eq!(report.completed[0].after, renamed);
    assert_eq!(report.completed[0].who(), "You (Reviewer)");
    assert!(o.history.pending_intents().unwrap().is_empty());
    assert_eq!(o.history.entries(None).unwrap(), report.completed);
    assert_eq!(read(root), renamed);
    assert_eq!(std::fs::read(root.join(OTHER)).unwrap(), other);

    let (_, config) = audit(&o.store);
    let actions: Vec<&str> = config
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        [
            "config.intent_opened",
            "config.intent_opened",
            "config.reconciled",
            "config.intent_dropped"
        ]
    );
    assert_eq!(
        config[2]["actor"],
        json!({"kind": "system", "component": "config-writer"})
    );
}

/// Review 5: a pending intent of the file is settled before the next write,
/// audited by the system as `config.reconciled`.
#[test]
fn config_history_pending_intent_is_settled_by_the_system_before_the_next_write() {
    let dir = project();
    let root = dir.path();
    let o = open(root);
    o.writer.fail_at(Some(FailPoint::AfterRename));
    o.writer
        .apply(reviewer(), set_per_run(json!(2)))
        .unwrap_err();
    o.writer.fail_at(None);
    o.writer.apply(reviewer(), set_per_run(json!(3))).unwrap();
    assert!(o.history.pending_intents().unwrap().is_empty());
    assert_eq!(o.history.entries(None).unwrap().len(), 2);

    let (_, config) = audit(&o.store);
    let actions: Vec<&str> = config
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        [
            "config.intent_opened",
            "config.reconciled",
            "config.intent_opened",
            "config.edited"
        ]
    );
    assert_eq!(
        config[1]["actor"],
        json!({"kind": "system", "component": "config-writer"})
    );
}
