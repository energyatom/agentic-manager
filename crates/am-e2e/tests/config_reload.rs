//! Story 1.9 acceptance test (R2, AD-16): the ticket's verify on a real
//! project store. A copy of `fixtures/projects/valid` is watched by a
//! `ConfigSession`; another process saves a seat file; exactly one
//! changed-on-disk event arrives with its diff and the snapshot hash is
//! unchanged until Reload; after Reload the new hash is live, History shows
//! the outside edit with the Actor and the audit log holds
//! `config.outside_edit` (no file content in it, chain verifies); the app's
//! own write raises no event; an invalid outside edit is reported with file,
//! line and fix and not applied. Written before the session exists.
#![forbid(unsafe_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_config::{ConfigEdit, ConfigEvent, ConfigSession, EditOp, ReloadError, load};
use am_domain::{Actor, ConfigHistory, HistoryKind};
use am_store::{Store, StoreConfigHistory, verify_jsonl};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SEAT: &str = "seats/alpha.md";
/// Stands for private file content: it must never reach the audit log.
const CANARY: &str = "am-config-canary-5d1a keep these instructions private";
const WAIT: Duration = Duration::from_secs(10);
const QUIET: Duration = Duration::from_millis(1500);

fn reviewer() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Reviewer".into(),
    }
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn read(root: &Path) -> String {
    String::from_utf8(std::fs::read(root.join(SEAT)).unwrap()).unwrap()
}

fn disk_hash(root: &Path) -> String {
    let loaded = load(root).unwrap();
    loaded
        .snapshot
        .unwrap_or_else(|| panic!("the project on disk is invalid: {:#?}", loaded.findings))
        .hash()
        .to_string()
}

/// Saves `text` over the seat file from another process, as an editor does
/// (temp file, then rename).
fn save_from_another_process(root: &Path, text: &str) {
    let dest = root.join(SEAT);
    let tmp = root.join("seats/.alpha.md.editor-swap");
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

fn actions(config: &[Value]) -> Vec<String> {
    config
        .iter()
        .map(|e| e["action"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn config_reload_outside_edit_waits_for_reload_on_a_real_store() {
    // The project, with a canary in the seat body.
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/projects/valid");
    am_testkit::copy_tree(&fixture, root);
    let seat = format!("{}{CANARY}\n", read(root));
    std::fs::write(root.join(SEAT), &seat).unwrap();

    // The runtime folder lives outside the project (AD-32).
    let runtime = tempfile::tempdir().unwrap();
    let layout = am_domain::RuntimeLayout::at(runtime.path().join("rt"));
    let (store, _) = am_store::open_runtime(&layout, "test-mac".into()).unwrap();
    let history = Arc::new(StoreConfigHistory::new(&store).unwrap());

    let (tx, events): (_, Receiver<ConfigEvent>) = channel();
    let tx = Mutex::new(tx);
    let on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync> = Arc::new(move |e| {
        if let Ok(tx) = tx.lock() {
            let _ = tx.send(e);
        }
    });
    let session = ConfigSession::open(root, history.clone(), on_event).expect("the session opens");
    let first_hash = session.snapshot().hash().to_string();
    assert_eq!(first_hash, disk_hash(root));
    let next = || match events.recv_timeout(WAIT) {
        Ok(e) => e,
        Err(e) => panic!("no event within {WAIT:?}: {e}"),
    };
    let quiet = |why: &str| match events.recv_timeout(QUIET) {
        Err(RecvTimeoutError::Timeout) => {}
        Ok(e) => panic!("{why}: an unexpected event {e:?}"),
        Err(e) => panic!("the event channel closed: {e}"),
    };

    // 1. Another editor saves the seat file: exactly one event with its diff.
    let edited = seat.replacen("per_run: 1.5", "per_run: 2", 1);
    save_from_another_process(root, &edited);
    #[allow(unreachable_patterns)]
    let diff = match next() {
        ConfigEvent::ChangedOnDisk {
            file,
            before_sha256,
            after_sha256,
            diff,
            findings,
            identity_change,
        } => {
            assert_eq!(file, SEAT);
            assert_eq!(before_sha256, Some(sha(seat.as_bytes())));
            assert_eq!(after_sha256, Some(sha(edited.as_bytes())));
            assert!(findings.is_empty(), "{findings:#?}");
            assert!(!identity_change);
            assert!(diff.contains("\n+  per_run: 2\n"), "{diff}");
            diff
        }
        other => panic!("expected ChangedOnDisk, got {other:?}"),
    };
    quiet("one outside save is announced once");
    assert_eq!(
        session.snapshot().hash(),
        first_hash,
        "nothing applies before Reload"
    );
    assert!(history.entries(None).unwrap().is_empty());
    let (_, config) = audit(&store);
    assert!(
        config.is_empty(),
        "nothing recorded before Reload: {config:#?}"
    );

    // 2. Reload: the new hash is live; History shows the outside edit.
    let entries = session.reload(reviewer()).expect("a valid edit reloads");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.kind, HistoryKind::OutsideEdit);
    assert_eq!(entry.file, SEAT);
    assert_eq!(entry.ref_entry, None);
    assert_eq!(entry.who(), "You (Reviewer)");
    assert_eq!(entry.diff, diff);
    assert_eq!(entry.before, seat.as_bytes());
    assert_eq!(entry.after, edited.as_bytes());
    let reloaded = session.snapshot().hash().to_string();
    assert_ne!(reloaded, first_hash);
    assert_eq!(reloaded, disk_hash(root), "the new hash is live");
    assert_eq!(history.entries(Some(SEAT.into())).unwrap(), entries);
    assert!(history.pending_intents().unwrap().is_empty());
    assert_eq!(read(root), edited, "Reload never rewrites the file");

    let (text, config) = audit(&store);
    assert!(
        !text.contains("am-config-canary"),
        "file content reached the audit log"
    );
    assert_eq!(
        actions(&config),
        ["config.intent_opened", "config.outside_edit"]
    );
    for e in &config {
        assert_eq!(e["target"], entry.id.as_str());
        assert_eq!(e["args"]["file"], SEAT);
        assert_eq!(e["args"]["kind"], "outside_edit");
        assert_eq!(
            e["actor"],
            json!({"kind": "human", "person": "owner", "hat": "Reviewer"})
        );
        for k in e["args"].as_object().unwrap().keys() {
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
        }
    }
    let again = session.reload(reviewer()).unwrap_err();
    assert!(
        matches!(again, ReloadError::NothingPending { .. }),
        "{again:?}"
    );

    // 3. The app's own write raises no event and moves the snapshot.
    let own = session
        .apply(
            reviewer(),
            ConfigEdit {
                file: SEAT.into(),
                ops: vec![EditOp::Set {
                    path: vec!["budget".into(), "per_run".into()],
                    value: json!(3),
                }],
                body: None,
            },
        )
        .expect("the edit applies");
    assert_eq!(own.kind, HistoryKind::Edit);
    quiet("the app's own write raises no event");
    let after_own = session.snapshot().hash().to_string();
    assert_ne!(after_own, reloaded);
    assert_eq!(after_own, disk_hash(root));

    // 4. An invalid outside edit is reported with file, line and fix and
    // never applied.
    let (_, before_invalid) = audit(&store);
    let invalid = read(root).replacen("team: core", "team: no-such-team", 1);
    save_from_another_process(root, &invalid);
    #[allow(unreachable_patterns)]
    match next() {
        ConfigEvent::ChangedOnDisk { file, findings, .. } => {
            assert_eq!(file, SEAT);
            assert!(
                findings.iter().any(|f| f.file == SEAT
                    && f.line == 5
                    && f.message.contains("no-such-team")
                    && !f.fix.is_empty()),
                "{findings:#?}"
            );
        }
        other => panic!("expected ChangedOnDisk, got {other:?}"),
    }
    let err = session.reload(reviewer()).unwrap_err();
    assert!(matches!(err, ReloadError::Invalid { .. }), "{err:?}");
    assert!(err.to_string().contains(SEAT), "{err}");
    assert!(err.to_string().contains("fix"), "{err}");
    assert_eq!(session.snapshot().hash(), after_own, "not applied");
    assert_eq!(history.entries(None).unwrap().len(), 2);
    assert!(history.pending_intents().unwrap().is_empty());
    let (_, after_invalid) = audit(&store);
    assert_eq!(
        after_invalid.len(),
        before_invalid.len(),
        "a refused Reload records nothing"
    );
}

/// Review fix (story 1.9): an empty file created outside has the same
/// content as no file, so it is never announced or applied, Reload has
/// nothing to do and the app's own edits are not refused over it; every
/// applied change keeps its History entry.
#[test]
fn config_reload_empty_file_created_outside_is_not_announced_or_applied() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/projects/valid");
    am_testkit::copy_tree(&fixture, root);
    let runtime = tempfile::tempdir().unwrap();
    let layout = am_domain::RuntimeLayout::at(runtime.path().join("rt"));
    let (store, _) = am_store::open_runtime(&layout, "test-mac".into()).unwrap();
    let history = Arc::new(StoreConfigHistory::new(&store).unwrap());
    let (tx, events): (_, Receiver<ConfigEvent>) = channel();
    let tx = Mutex::new(tx);
    let on_event: Arc<dyn Fn(ConfigEvent) + Send + Sync> = Arc::new(move |e| {
        if let Ok(tx) = tx.lock() {
            let _ = tx.send(e);
        }
    });
    let session = ConfigSession::open(root, history.clone(), on_event).expect("the session opens");
    let first = session.snapshot().hash().to_string();

    std::fs::write(root.join("skills/brief/references/notes.txt"), b"").unwrap();
    match events.recv_timeout(QUIET) {
        Err(RecvTimeoutError::Timeout) => {}
        other => panic!("an empty new file is not announced: {other:?}"),
    }
    let err = session.reload(reviewer()).unwrap_err();
    assert!(matches!(err, ReloadError::NothingPending { .. }), "{err:?}");
    assert_eq!(session.snapshot().hash(), first);
    assert!(history.entries(None).unwrap().is_empty());
    assert!(history.pending_intents().unwrap().is_empty());

    let own = session
        .apply(
            reviewer(),
            ConfigEdit {
                file: SEAT.into(),
                ops: vec![EditOp::Set {
                    path: vec!["budget".into(), "per_run".into()],
                    value: json!(2),
                }],
                body: None,
            },
        )
        .expect("the app's edit is not refused over an empty file");
    assert_eq!(own.kind, HistoryKind::Edit);
    assert_eq!(history.entries(None).unwrap(), vec![own]);
}
