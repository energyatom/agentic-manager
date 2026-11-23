//! Epic 3 story 3, owner condition 3 (AD-15; matrix row 33): agents have
//! no config path. Besides having no tool, route or Proposal that reaches
//! config in this story, the am-config writer itself refuses every config
//! write whose actor is `Actor::Seat` or `Actor::Assistant`: an edit (a
//! `limits:` raise above all), a create, an Undo and a Restore. The refusal
//! is in words naming the actor and comes before anything is written: the
//! file keeps its bytes, and no History intent or entry is recorded. The
//! same write by `Actor::Human` applies. Written before the check exists
//! (today the writer applies the edit for any actor).
#![forbid(unsafe_code)]

mod common;

use std::path::Path;
use std::sync::Arc;

use am_config::{ConfigCreate, ConfigEdit, ConfigWriter, EditOp};
use am_domain::{Actor, ConfigHistory};
use am_testkit::InMemoryConfigHistory;
use common::copy_of;
use serde_json::json;
use uuid::Uuid;

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

/// The agents: a seat inside a step, and the Assistant through a proposal.
fn agents() -> Vec<Actor> {
    let run = Uuid::now_v7();
    vec![
        Actor::Seat {
            seat: "first".into(),
            run,
            step: "work".into(),
        },
        Actor::Assistant { proposal: run },
    ]
}

fn writer(root: &Path) -> (ConfigWriter, Arc<InMemoryConfigHistory>) {
    let history = Arc::new(InMemoryConfigHistory::default());
    let w = ConfigWriter::open(root, history.clone()).unwrap();
    (w, history)
}

fn set(path: &[&str], value: serde_json::Value) -> EditOp {
    EditOp::Set {
        path: path.iter().map(|s| s.to_string()).collect(),
        value,
    }
}

/// What a refused write left behind: the files' bytes, and History's
/// intents and entries.
fn state(
    root: &Path,
    files: &[&str],
    history: &InMemoryConfigHistory,
) -> (Vec<Vec<u8>>, usize, usize) {
    (
        files
            .iter()
            .map(|f| std::fs::read(root.join(f)).unwrap_or_default())
            .collect(),
        history.pending_intents().unwrap().len(),
        history.entries(None).unwrap().len(),
    )
}

/// The edits an agent might want: raising a step's cap (the fixture's
/// `capped` workflow) and changing a seat's words.
fn edits() -> Vec<ConfigEdit> {
    vec![
        ConfigEdit {
            file: "workflows/capped.yaml".into(),
            ops: vec![set(&["steps", "0", "limits", "iterations"], json!(50))],
            body: None,
        },
        ConfigEdit {
            file: "seats/first.md".into(),
            ops: vec![set(&["description"], json!("Writes whatever it likes."))],
            body: None,
        },
    ]
}

#[test]
fn a_seat_or_assistant_actor_cannot_edit_config() {
    for edit in edits() {
        let dir = copy_of("hand-offs");
        let root = dir.path();
        let (w, history) = writer(root);
        let files = [edit.file.as_str()];
        let before = state(root, &files, &history);

        for actor in agents() {
            let refused = w
                .apply(actor.clone(), edit.clone())
                .expect_err("an agent never writes config");
            let words = refused.to_string();
            assert!(words.contains("refused"), "{}: {words}", edit.file);
            assert!(
                words.contains(&actor.describe()),
                "{}: names who asked: {words}",
                edit.file
            );
            assert_eq!(
                state(root, &files, &history),
                before,
                "{}: nothing written for {actor:?}",
                edit.file
            );
        }

        // The same edit by the owner applies.
        let entry = w.apply(owner(), edit.clone()).unwrap();
        assert_eq!(entry.file, edit.file);
        assert_ne!(state(root, &files, &history).0, before.0, "{}", edit.file);
    }
}

/// A create, an Undo and a Restore by an agent are refused the same way;
/// the owner's apply.
#[test]
fn a_seat_or_assistant_actor_cannot_create_undo_or_restore() {
    let dir = copy_of("hand-offs");
    let root = dir.path();
    let (w, history) = writer(root);
    let new_file = "workflows/extra.yaml";
    let create = ConfigCreate {
        file: new_file.into(),
        bytes: b"schema_version: 2\nname: Extra\nteam: core\nsteps:\n  - id: work\n    seat: first\n    instruction: Do more work.\n".to_vec(),
        audit: vec![],
    };
    let edit = edits().remove(1);
    let files = [new_file, edit.file.as_str()];
    let entry = w.apply(owner(), edit.clone()).unwrap();
    let before = state(root, &files, &history);
    assert!(!root.join(new_file).exists());

    for actor in agents() {
        for (what, result) in [
            (
                "create",
                w.create(actor.clone(), create.clone()).map(|_| ()),
            ),
            ("undo", w.undo(actor.clone(), &entry.id).map(|_| ())),
            ("restore", w.restore(actor.clone(), &entry.id).map(|_| ())),
        ] {
            let refused = result.expect_err(what);
            assert!(
                refused.to_string().contains(&actor.describe()),
                "{what}: {refused}"
            );
            assert_eq!(state(root, &files, &history), before, "{what}: {actor:?}");
        }
        assert!(!root.join(new_file).exists());
    }

    w.create(owner(), create).unwrap();
    assert!(root.join(new_file).exists());
    w.undo(owner(), &entry.id).unwrap();
}
