//! RULE-16 trust test (AD-25, story 1.7): the audit log is append-only and
//! tamper-evident, and it holds no row content or secrets.
//!
//! - A raw UPDATE or DELETE on `audit_log` through a read-write SQLite
//!   connection is refused and changes nothing.
//! - The JSON Lines export of a project verifies; altering one line makes
//!   the chain check name that entry and line.
//! - A step output and a secret canary (from a `FakeSecretStore`) written as
//!   step output appear nowhere in the export; only their hashes do.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use am_domain::{Actor, SecretStore, Usage};
use am_store::{OpenReport, Store, StoreError, TriggerRow, WriteCommand, verify_jsonl};
use am_testkit::FakeSecretStore;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

const CONTENT_CANARY: &str = "am-content-canary-5d1e83 the step's private answer";
const SECRET_ACCOUNT: &str = "rule16-account";
const SECRET_CANARY: &str = "am-secret-canary-9b7f20";

fn sys() -> Actor {
    Actor::System {
        component: "rule16".into(),
    }
}

/// The runtime folder the store opens (story 1.13): `rt/` in `root`.
fn layout(root: &Path) -> am_domain::RuntimeLayout {
    am_domain::RuntimeLayout::at(root.join("rt"))
}

fn db(root: &Path) -> PathBuf {
    layout(root).state_db()
}

fn open_project(root: &Path) -> Result<(Store, OpenReport), StoreError> {
    am_store::open_runtime(&layout(root), "test-mac".into())
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// A project after a run (with a model call settled and two step outputs:
/// one plain content, one a secret canary), a trigger with a fire, and a
/// free-text audit entry carrying a hash.
fn populated(root: &Path) -> Store {
    let (store, _) = open_project(root).unwrap();
    let w = store.writer();
    let secrets = FakeSecretStore::with(SECRET_ACCOUNT, SECRET_CANARY);
    let secret = secrets.get(SECRET_ACCOUNT).unwrap().unwrap();
    w.exec(
        sys(),
        WriteCommand::CreateRun {
            run_id: "run-1".into(),
            workflow: "wf-1".into(),
            origin: "manual".into(),
            steps: vec!["a".into(), "b".into()],
        },
    )
    .unwrap();
    let steps = store.reader().unwrap().steps("run-1").unwrap();
    let (a, b) = (steps[0].id.clone(), steps[1].id.clone());
    w.exec(
        sys(),
        WriteCommand::StartStep {
            step_run_id: a.clone(),
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::ReserveCall {
            step_run_id: a.clone(),
            call_id: "call-1".into(),
            call_seq: 1,
            reserved_micro_usd: 5000,
            rate_snapshot: "rates-1".into(),
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::SettleAndFinishStep {
            call_id: "call-1".into(),
            usage: Some(Usage {
                input_tokens: 11,
                output_tokens: 7,
            }),
            cost_micro_usd: Some(420),
            step_run_id: a,
            status: "succeeded".into(),
            output: Some(CONTENT_CANARY.into()),
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::StartStep {
            step_run_id: b.clone(),
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::FinishStep {
            step_run_id: b,
            status: "succeeded".into(),
            output: Some(secret.expose().to_string()),
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
        WriteCommand::UpsertTrigger {
            trigger: TriggerRow {
                id: "trigger-1".into(),
                cron: "0 9 * * *".into(),
                tz: "UTC".into(),
                catch_up: "once".into(),
                state: "active".into(),
                next_fire: Some("2026-11-21T09:00:00.000Z".into()),
                last_fired: None,
            },
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::RecordFire {
            trigger_id: "trigger-1".into(),
            scheduled_for: "2026-11-21T09:00:00.000Z".into(),
            kind: "on_time".into(),
            next_fire: "2026-11-22T09:00:00.000Z".into(),
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::Audit {
            action: "folder.read".into(),
            target: "root-1:notes/a.md".into(),
            outcome: "ok".into(),
            hash: Some(sha256_hex("the file's bytes")),
        },
    )
    .unwrap();
    store
}

fn export(store: &Store) -> (String, u64, String) {
    let mut out = Vec::new();
    let (count, head) = store
        .reader()
        .unwrap()
        .export_audit_jsonl(&mut out)
        .unwrap();
    (String::from_utf8(out).unwrap(), count, head)
}

#[test]
fn raw_update_and_delete_on_audit_log_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (before, _, _) = export(&store);

    let rw = Connection::open(db(dir.path())).unwrap();
    let upd = rw
        .execute(
            "UPDATE audit_log SET target = 'rewritten' WHERE seq = 2",
            [],
        )
        .unwrap_err();
    assert!(
        upd.to_string().contains("audit_log is append-only"),
        "{upd}"
    );
    let del = rw.execute("DELETE FROM audit_log", []).unwrap_err();
    assert!(
        del.to_string().contains("audit_log is append-only"),
        "{del}"
    );
    drop(rw);

    let (after, _, _) = export(&store);
    assert_eq!(before, after, "rows unchanged");
}

#[test]
fn clean_export_verifies_with_its_count_and_head() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, count, head) = export(&store);

    assert_eq!(text.lines().count() as u64, count);
    let ok = verify_jsonl(text.as_bytes()).unwrap();
    assert_eq!(ok.entries, count);
    assert_eq!(ok.head, head);
    // Migrations, the run, the settle, the trigger and the free-text entry
    // are all in the log.
    for action in [
        "\"store.migrated\"",
        "\"run.created\"",
        "\"model_call.settled+step.finished\"",
        "\"trigger.fired\"",
        "\"folder.read\"",
    ] {
        assert!(text.contains(action), "{action} missing from:\n{text}");
    }
}

/// Edits field `key` of the JSON object on line `index` (0-based).
fn alter(text: &str, index: usize, key: &str, value: serde_json::Value) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut obj: serde_json::Value = serde_json::from_str(&lines[index]).unwrap();
    obj[key] = value;
    lines[index] = serde_json::to_string(&obj).unwrap();
    lines.join("\n") + "\n"
}

fn seq_of(text: &str, index: usize) -> i64 {
    let obj: serde_json::Value = serde_json::from_str(text.lines().nth(index).unwrap()).unwrap();
    obj["seq"].as_i64().unwrap()
}

#[test]
fn altering_one_line_names_that_entry() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, _, _) = export(&store);
    let lines: Vec<&str> = text.lines().collect();
    let middle = lines.len() / 2;

    // A middle line's target.
    let bad = alter(&text, middle, "target", "someone-else".into());
    let brk = verify_jsonl(bad.as_bytes()).unwrap_err();
    assert_eq!(brk.line, middle as u64 + 1);
    assert_eq!(brk.seq, Some(seq_of(&text, middle)));
    let msg = brk.to_string();
    assert!(msg.contains(&format!("line {}", middle + 1)), "{msg}");
    assert!(
        msg.contains(&format!("entry {}", seq_of(&text, middle))),
        "{msg}"
    );

    // A middle line's args (the first entry after the migrations that has some).
    let with_args = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["args"].is_object()
        })
        .map(|(i, _)| i)
        .expect("an entry with args");
    let bad = alter(
        &text,
        with_args,
        "args",
        serde_json::json!({"status": "failed"}),
    );
    let brk = verify_jsonl(bad.as_bytes()).unwrap_err();
    assert_eq!(brk.line, with_args as u64 + 1);
    assert_eq!(brk.seq, Some(seq_of(&text, with_args)));
}

#[test]
fn no_row_content_or_secret_canary_in_the_export() {
    let dir = tempfile::tempdir().unwrap();
    let store = populated(dir.path());
    let (text, _, _) = export(&store);

    // The content is in the journal (deleted with the run), never in the log.
    let steps = store.reader().unwrap().steps("run-1").unwrap();
    assert_eq!(steps[0].output.as_deref(), Some(CONTENT_CANARY));
    for canary in [CONTENT_CANARY, "am-content-canary", SECRET_CANARY] {
        assert!(!text.contains(canary), "`{canary}` leaked into:\n{text}");
    }
    // Their hashes stand in for them.
    assert!(text.contains(&sha256_hex(CONTENT_CANARY)), "{text}");
    assert!(text.contains(&sha256_hex(SECRET_CANARY)), "{text}");
    // Nor is either anywhere in audit_log's stored text.
    let c = Connection::open(db(dir.path())).unwrap();
    let rows: Vec<String> = c
        .prepare("SELECT at || actor || action || target || outcome || coalesce(args, '') FROM audit_log")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(!rows.is_empty());
    for row in rows {
        assert!(!row.contains("am-content-canary"), "{row}");
        assert!(!row.contains(SECRET_CANARY), "{row}");
    }
}
