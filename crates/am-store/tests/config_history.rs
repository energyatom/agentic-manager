//! Story 1.8 (am-store side): config History (AD-16, R3). The intent row,
//! the History entry and their audit entries, through the writer commands
//! and through `StoreConfigHistory`, the `ConfigHistory` adapter. Written
//! before the tables exist.
#![forbid(unsafe_code)]

use am_domain::{Actor, ConfigHistory, HistoryError, HistoryKind, NewIntent};
use am_store::{Store, StoreConfigHistory, WriteCommand, verify_jsonl};
use rusqlite::Connection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Text that must never reach the audit log (it stands for file content).
const CANARY: &str = "am-config-canary-41c7 private instructions";

fn reviewer() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Reviewer".into(),
    }
}

fn sys() -> Actor {
    Actor::System {
        component: "config-writer".into(),
    }
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn intent(file: &str, kind: HistoryKind, ref_entry: Option<String>, n: u32) -> NewIntent {
    let before = format!("---\nschema_version: 1\nname: A{n}\n---\n{CANARY}\n").into_bytes();
    let after = format!("---\nschema_version: 1\nname: B{n}\n---\n{CANARY}\n").into_bytes();
    NewIntent {
        file: file.into(),
        kind,
        ref_entry,
        before_sha256: sha(&before),
        after_sha256: sha(&after),
        diff: format!(
            "--- a/{file}\n+++ b/{file}\n@@ -3 +3 @@\n-name: A{n}\n+name: B{n}\n{CANARY}\n"
        ),
        before,
        after,
    }
}

fn open() -> (tempfile::TempDir, Store, StoreConfigHistory) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("state.db")).unwrap();
    let history = StoreConfigHistory::new(&store).unwrap();
    (dir, store, history)
}

/// The audit log as exported JSON objects, after checking the chain.
fn audit(store: &Store) -> Vec<Value> {
    let mut out = Vec::new();
    store
        .reader()
        .unwrap()
        .export_audit_jsonl(&mut out)
        .unwrap();
    verify_jsonl(out.as_slice()).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(
        !text.contains("am-config-canary"),
        "content reached the audit log"
    );
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn config_entries(store: &Store) -> Vec<Value> {
    audit(store)
        .into_iter()
        .filter(|e| e["action"].as_str().unwrap().starts_with("config."))
        .collect()
}

/// A config entry's `args` hold only ids, the file path, hashes and the kind.
fn assert_args_redacted(e: &Value) {
    let args = e["args"].as_object().unwrap_or_else(|| panic!("{e}"));
    for key in args.keys() {
        assert!(
            [
                "after_sha256",
                "before_sha256",
                "entry_id",
                "file",
                "kind",
                "ref_entry"
            ]
            .contains(&key.as_str()),
            "unexpected args key {key} in {e}"
        );
    }
}

#[test]
fn config_history_commit_moves_the_intent_into_history_with_its_audit_entry() {
    let (_dir, store, history) = open();
    let i = intent("seats/alpha.md", HistoryKind::Edit, None, 1);
    let id = history.begin_intent(reviewer(), i.clone()).unwrap();
    assert_eq!(id.len(), 36, "a UUID: {id}");

    let pending = history.pending_intents().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, id);
    assert_eq!(pending[0].actor, reviewer());
    assert_eq!(pending[0].intent, i);
    assert!(history.entries(None).unwrap().is_empty());

    let entry = history.commit_entry(reviewer(), id.clone()).unwrap();
    assert_eq!(entry.id, id);
    assert_eq!(entry.kind, HistoryKind::Edit);
    assert_eq!(entry.file, "seats/alpha.md");
    assert_eq!(entry.who(), "You (Reviewer)");
    assert_eq!(
        (entry.before.clone(), entry.after.clone()),
        (i.before, i.after)
    );
    assert_eq!(entry.before_sha256, i.before_sha256);
    assert_eq!(entry.after_sha256, i.after_sha256);
    assert_eq!(entry.diff, i.diff);
    assert!(entry.at.ends_with('Z'));
    assert!(history.pending_intents().unwrap().is_empty());
    assert_eq!(history.entries(None).unwrap(), vec![entry.clone()]);
    assert_eq!(history.entry(id.clone()).unwrap(), Some(entry.clone()));

    let log = config_entries(&store);
    let actions: Vec<&str> = log.iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert_eq!(actions, ["config.intent_opened", "config.edited"]);
    for e in &log {
        assert_eq!(e["target"], id.as_str());
        assert_eq!(e["args"]["file"], "seats/alpha.md");
        assert_eq!(e["args"]["kind"], "edit");
        assert_eq!(e["args"]["before_sha256"], entry.before_sha256.as_str());
        assert_eq!(e["args"]["after_sha256"], entry.after_sha256.as_str());
        assert_args_redacted(e);
    }
    assert_eq!(log[1]["args"]["entry_id"], id.as_str());
}

#[test]
fn config_history_undo_and_restore_name_their_entry_and_audit_action() {
    let (_dir, store, history) = open();
    let e1 = history
        .begin_intent(
            reviewer(),
            intent("company.yaml", HistoryKind::Edit, None, 1),
        )
        .and_then(|id| history.commit_entry(reviewer(), id))
        .unwrap();
    let e2 = history
        .begin_intent(
            reviewer(),
            intent("company.yaml", HistoryKind::Undo, Some(e1.id.clone()), 2),
        )
        .and_then(|id| history.commit_entry(reviewer(), id))
        .unwrap();
    let e3 = history
        .begin_intent(
            reviewer(),
            intent("company.yaml", HistoryKind::Restore, Some(e1.id.clone()), 3),
        )
        .and_then(|id| history.commit_entry(reviewer(), id))
        .unwrap();
    assert_eq!(e2.kind, HistoryKind::Undo);
    assert_eq!(e2.ref_entry.as_deref(), Some(e1.id.as_str()));
    assert_eq!(e3.kind, HistoryKind::Restore);
    assert!(e1.seq < e2.seq && e2.seq < e3.seq);

    let done: Vec<(String, String)> = config_entries(&store)
        .iter()
        .filter(|e| e["action"] != "config.intent_opened")
        .map(|e| {
            (
                e["action"].as_str().unwrap().to_string(),
                e["args"]["ref_entry"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    assert_eq!(
        done,
        [
            ("config.edited".to_string(), String::new()),
            ("config.undone".to_string(), e1.id.clone()),
            ("config.restored".to_string(), e1.id.clone()),
        ]
    );

    // An Undo of an entry that does not exist is refused.
    let err = history
        .begin_intent(
            reviewer(),
            intent("company.yaml", HistoryKind::Undo, Some("nope".into()), 4),
        )
        .unwrap_err();
    assert!(matches!(err, HistoryError::Store(_)), "{err:?}");
}

#[test]
fn config_history_drop_and_reconcile() {
    let (_dir, store, history) = open();
    let a = history
        .begin_intent(reviewer(), intent("seats/a.md", HistoryKind::Edit, None, 1))
        .unwrap();
    let b = history
        .begin_intent(reviewer(), intent("seats/b.md", HistoryKind::Edit, None, 2))
        .unwrap();
    assert_eq!(history.pending_intents().unwrap().len(), 2);

    history.drop_intent(sys(), a.clone()).unwrap();
    let done = history.reconcile_intent(sys(), b.clone()).unwrap();
    assert_eq!(done.id, b);
    assert_eq!(done.actor, reviewer(), "the entry keeps the edit's Actor");
    assert!(history.pending_intents().unwrap().is_empty());
    assert_eq!(history.entries(None).unwrap(), vec![done.clone()]);

    let log = config_entries(&store);
    let tail: Vec<(&str, &str)> = log[2..]
        .iter()
        .map(|e| (e["action"].as_str().unwrap(), e["target"].as_str().unwrap()))
        .collect();
    assert_eq!(
        tail,
        [
            ("config.intent_dropped", a.as_str()),
            ("config.reconciled", b.as_str())
        ]
    );
    for e in &log[2..] {
        assert_eq!(
            e["actor"],
            json!({"kind": "system", "component": "config-writer"})
        );
        assert_args_redacted(e);
    }

    // Unknown or already-finished intents are refused and audit nothing.
    let before = audit(&store).len();
    for r in [
        history.commit_entry(reviewer(), a.clone()).map(|_| ()),
        history.reconcile_intent(sys(), b.clone()).map(|_| ()),
        history.drop_intent(sys(), "nope".into()),
    ] {
        assert!(matches!(r, Err(HistoryError::NoIntent(_))), "{r:?}");
    }
    assert_eq!(audit(&store).len(), before);
}

#[test]
fn config_history_refuses_hashes_that_do_not_match_the_bytes() {
    let (_dir, store, history) = open();
    let before = audit(&store).len();
    let mut wrong = intent("seats/a.md", HistoryKind::Edit, None, 1);
    wrong.after_sha256 = wrong.before_sha256.clone();
    assert!(history.begin_intent(reviewer(), wrong).is_err());
    let mut not_hex = intent("seats/a.md", HistoryKind::Edit, None, 1);
    not_hex.before_sha256 = format!("{CANARY:<64}");
    assert!(history.begin_intent(reviewer(), not_hex).is_err());
    let mut ref_on_edit = intent("seats/a.md", HistoryKind::Edit, Some("x".into()), 1);
    ref_on_edit.ref_entry = Some("x".into());
    assert!(history.begin_intent(reviewer(), ref_on_edit).is_err());
    let undo_without_ref = intent("seats/a.md", HistoryKind::Undo, None, 1);
    assert!(history.begin_intent(reviewer(), undo_without_ref).is_err());
    assert!(history.pending_intents().unwrap().is_empty());
    assert_eq!(audit(&store).len(), before);
}

#[test]
fn config_history_filters_by_file_and_is_append_only() {
    let (dir, store, history) = open();
    for (n, file) in ["seats/a.md", "company.yaml", "seats/a.md"]
        .iter()
        .enumerate()
    {
        let id = history
            .begin_intent(reviewer(), intent(file, HistoryKind::Edit, None, n as u32))
            .unwrap();
        history.commit_entry(reviewer(), id).unwrap();
    }
    assert_eq!(history.entries(None).unwrap().len(), 3);
    let a = history.entries(Some("seats/a.md".into())).unwrap();
    assert_eq!(a.len(), 2);
    assert!(a[0].seq < a[1].seq);
    assert_eq!(history.entry("nope".into()).unwrap(), None);

    // The writer command path is the only way in: raw edits are refused.
    let rw = Connection::open(dir.path().join("state.db")).unwrap();
    assert!(
        rw.execute("UPDATE config_history SET diff = 'x'", [])
            .is_err()
    );
    assert!(rw.execute("DELETE FROM config_history", []).is_err());
    drop(rw);
    assert_eq!(history.entries(None).unwrap().len(), 3);

    // Every command needs an Actor and is audited; the chain verifies.
    store
        .writer()
        .exec(
            sys(),
            WriteCommand::DropConfigIntent {
                intent_id: "nope".into(),
            },
        )
        .unwrap_err();
    assert_eq!(config_entries(&store).len(), 6);
}

// ---------------------------------------------------------------- story 1.9

/// An outside edit's intent (story 1.9): `before` or `after` may be empty for
/// a file created or deleted outside the app.
fn outside(file: &str, before: &[u8], after: &[u8]) -> NewIntent {
    NewIntent {
        file: file.into(),
        kind: HistoryKind::OutsideEdit,
        ref_entry: None,
        before_sha256: sha(before),
        after_sha256: sha(after),
        before: before.to_vec(),
        after: after.to_vec(),
        diff: format!("--- a/{file}\n+++ b/{file}\n@@ -1 +1 @@\n{CANARY}\n"),
    }
}

/// Story 1.9: `OutsideEdit` is stored as `outside_edit`, takes no ref, reads
/// back from the table and is audited as `config.outside_edit` with the
/// Reload's Actor.
#[test]
fn config_history_outside_edit_takes_no_ref_and_is_audited_as_config_outside_edit() {
    assert_eq!(HistoryKind::OutsideEdit.as_str(), "outside_edit");
    assert_eq!(
        HistoryKind::parse("outside_edit"),
        Some(HistoryKind::OutsideEdit)
    );
    assert_eq!(
        serde_json::to_value(HistoryKind::OutsideEdit).unwrap(),
        json!("outside_edit")
    );

    let (_dir, store, history) = open();
    let i = intent("seats/alpha.md", HistoryKind::OutsideEdit, None, 1);
    let id = history.begin_intent(reviewer(), i.clone()).unwrap();
    assert_eq!(history.pending_intents().unwrap()[0].intent, i);
    let entry = history.commit_entry(reviewer(), id.clone()).unwrap();
    assert_eq!(entry.kind, HistoryKind::OutsideEdit);
    assert_eq!(entry.ref_entry, None);
    assert_eq!(entry.who(), "You (Reviewer)");
    assert_eq!(entry.diff, i.diff);
    assert_eq!(history.entries(None).unwrap(), vec![entry.clone()]);
    assert_eq!(history.entry(id.clone()).unwrap(), Some(entry.clone()));

    let log = config_entries(&store);
    let actions: Vec<&str> = log.iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert_eq!(actions, ["config.intent_opened", "config.outside_edit"]);
    for e in &log {
        assert_eq!(e["target"], id.as_str());
        assert_eq!(e["args"]["file"], "seats/alpha.md");
        assert_eq!(e["args"]["kind"], "outside_edit");
        assert_eq!(
            e["actor"],
            json!({"kind": "human", "person": "owner", "hat": "Reviewer"})
        );
        assert_args_redacted(e);
    }
    assert_eq!(log[1]["args"]["entry_id"], id.as_str());

    // An outside edit names no earlier entry.
    let before = audit(&store).len();
    let with_ref = intent(
        "seats/alpha.md",
        HistoryKind::OutsideEdit,
        Some(entry.id.clone()),
        2,
    );
    assert!(history.begin_intent(reviewer(), with_ref).is_err());
    assert!(history.pending_intents().unwrap().is_empty());
    assert_eq!(audit(&store).len(), before);
}

/// Story 1.9: a file created outside (no before-bytes) and one deleted
/// outside (no after-bytes) are both History entries.
#[test]
fn config_history_outside_edit_records_created_and_deleted_files() {
    let (_dir, store, history) = open();
    let workflow = b"schema_version: 1\nname: X\n";
    let seat = format!("---\nschema_version: 1\nname: Gamma\n---\n{CANARY}\n").into_bytes();
    let created = history
        .begin_intent(reviewer(), outside("workflows/x.yaml", b"", workflow))
        .and_then(|id| history.commit_entry(reviewer(), id))
        .unwrap();
    let deleted = history
        .begin_intent(reviewer(), outside("seats/gamma.md", &seat, b""))
        .and_then(|id| history.commit_entry(reviewer(), id))
        .unwrap();
    assert!(created.before.is_empty());
    assert_eq!(created.before_sha256, sha(b""));
    assert_eq!(created.after, workflow);
    assert_eq!(deleted.before, seat);
    assert!(deleted.after.is_empty());
    assert_eq!(deleted.after_sha256, sha(b""));
    assert_eq!(
        history.entries(None).unwrap(),
        vec![created.clone(), deleted.clone()]
    );
    let done: Vec<(String, String)> = config_entries(&store)
        .iter()
        .filter(|e| e["action"] != "config.intent_opened")
        .map(|e| {
            (
                e["action"].as_str().unwrap().to_string(),
                e["target"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        done,
        [
            ("config.outside_edit".to_string(), created.id.clone()),
            ("config.outside_edit".to_string(), deleted.id.clone()),
        ]
    );
}

// --------------------------------------------------------------- story 1.10

/// Story 1.10: `Migration` is stored as `migration`, takes no ref (as an
/// outside edit), reads back from the table and is audited as
/// `config.migrated` with the accept's Actor; a crash after the rename is
/// still settled by `config.reconciled`.
#[test]
fn config_history_migration_takes_no_ref_and_is_audited_as_config_migrated() {
    assert_eq!(HistoryKind::Migration.as_str(), "migration");
    assert_eq!(
        HistoryKind::parse("migration"),
        Some(HistoryKind::Migration)
    );
    assert_eq!(
        serde_json::to_value(HistoryKind::Migration).unwrap(),
        json!("migration")
    );

    let (_dir, store, history) = open();
    let i = intent("seats/alpha.md", HistoryKind::Migration, None, 1);
    let id = history.begin_intent(reviewer(), i.clone()).unwrap();
    assert_eq!(history.pending_intents().unwrap()[0].intent, i);
    let entry = history.commit_entry(reviewer(), id.clone()).unwrap();
    assert_eq!(entry.kind, HistoryKind::Migration);
    assert_eq!(entry.ref_entry, None);
    assert_eq!(entry.who(), "You (Reviewer)");
    assert_eq!(entry.diff, i.diff);
    assert_eq!(
        (entry.before.clone(), entry.after.clone()),
        (i.before, i.after)
    );
    assert_eq!(history.entries(None).unwrap(), vec![entry.clone()]);
    assert_eq!(history.entry(id.clone()).unwrap(), Some(entry.clone()));

    let log = config_entries(&store);
    let actions: Vec<&str> = log.iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert_eq!(actions, ["config.intent_opened", "config.migrated"]);
    for e in &log {
        assert_eq!(e["target"], id.as_str());
        assert_eq!(e["args"]["file"], "seats/alpha.md");
        assert_eq!(e["args"]["kind"], "migration");
        assert_eq!(
            e["actor"],
            json!({"kind": "human", "person": "owner", "hat": "Reviewer"})
        );
        assert_args_redacted(e);
    }
    assert_eq!(log[1]["args"]["entry_id"], id.as_str());

    // A migration names no earlier entry.
    let before = audit(&store).len();
    let with_ref = intent(
        "seats/alpha.md",
        HistoryKind::Migration,
        Some(entry.id.clone()),
        2,
    );
    assert!(history.begin_intent(reviewer(), with_ref).is_err());
    assert!(history.pending_intents().unwrap().is_empty());
    assert_eq!(audit(&store).len(), before);

    // A migration intent left by a crash after the rename is reconciled.
    let left = history
        .begin_intent(
            reviewer(),
            intent("workflows/digest.yaml", HistoryKind::Migration, None, 3),
        )
        .unwrap();
    let done = history.reconcile_intent(sys(), left.clone()).unwrap();
    assert_eq!(done.kind, HistoryKind::Migration);
    assert_eq!(done.actor, reviewer(), "the entry keeps the accept's Actor");
    let last = config_entries(&store).pop().unwrap();
    assert_eq!(last["action"], "config.reconciled");
    assert_eq!(last["args"]["kind"], "migration");
}
