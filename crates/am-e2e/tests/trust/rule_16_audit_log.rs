//! RULE-16 trust test (AD-25, story 1.7): the audit log is append-only and
//! tamper-evident, and it holds no row content or secrets.
//!
//! - A raw UPDATE or DELETE on `audit_log` through a read-write SQLite
//!   connection is refused and changes nothing.
//! - The JSON Lines export of a project verifies; altering one line makes
//!   the chain check name that entry and line.
//! - A step output and a secret canary (from a `FakeSecretStore`) written as
//!   step output appear nowhere in the export; only their hashes do.
//! - Story 3.4 (AD-24, AD-25): a resolution is one `inbox.resolved` entry
//!   by "You (Reviewer)" holding the note's hash, never the note; the
//!   engine's apply follows as `System (engine)`.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use am_domain::{Actor, Decision, HumanActor, InboxKind, REVIEWER_HAT, SecretStore, Taint, Usage};
use am_store::{
    NewInboxItem, OpenReport, StepInput, Store, StoreError, TriggerRow, WriteCommand, verify_jsonl,
};
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
            // Story 3.1: the run pins its config snapshot's hash.
            config_hash: Some(sha256_hex("the config files")),
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
            // Story 3.3: the attempt's taint, raised (never lowered) with
            // the reservation (AD-10).
            taint: Taint::Clean,
            purpose: am_store::CallPurpose::Turn,
            prompt: vec![],
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
            args: Default::default(),
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

/// Story 3.4, row 10: a return at a gate, resolved by the owner as
/// Reviewer and applied by the engine, is audited once as
/// `inbox.resolved`: actor `{"kind":"human","person":"owner","hat":
/// "Reviewer"}` (`describe()` "You (Reviewer)"), target the item, args the
/// kind, the decision, the step run, the taint and the note's sha256. The
/// note's text is nowhere in the export or in `audit_log`; the apply's
/// entries are `System (engine)`'s; the export verifies.
#[test]
fn a_resolution_is_audited_as_you_reviewer_without_the_note() {
    const NOTE_CANARY: &str = "am-note-canary-41b9 tighten the opening";
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open_project(dir.path()).unwrap();
    let w = store.writer();
    let engine = Actor::System {
        component: "engine".into(),
    };
    w.exec(
        engine.clone(),
        WriteCommand::CreateRun {
            run_id: "run-g".into(),
            workflow: "wf-g".into(),
            origin: "manual".into(),
            steps: vec!["draft".into(), "approve".into(), "publish".into()],
            config_hash: Some(sha256_hex("the config files")),
        },
    )
    .unwrap();
    let steps = store.reader().unwrap().steps("run-g").unwrap();
    let (draft, approve) = (steps[0].id.clone(), steps[1].id.clone());
    w.exec(
        engine.clone(),
        WriteCommand::StartAttempt {
            step_run_id: draft.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
        },
    )
    .unwrap();
    w.exec(
        engine.clone(),
        WriteCommand::ReserveCall {
            step_run_id: draft.clone(),
            call_id: "call-g".into(),
            call_seq: 1,
            reserved_micro_usd: 0,
            rate_snapshot: "none".into(),
            taint: Taint::Clean,
            purpose: am_store::CallPurpose::Turn,
            prompt: vec![],
        },
    )
    .unwrap();
    w.exec(
        engine.clone(),
        WriteCommand::SettleAndFinishStep {
            call_id: "call-g".into(),
            usage: None,
            cost_micro_usd: None,
            step_run_id: draft.clone(),
            status: "succeeded".into(),
            output: Some("the version".into()),
        },
    )
    .unwrap();
    w.exec(
        engine.clone(),
        WriteCommand::PauseStep {
            step_run_id: approve.clone(),
            inputs: vec![StepInput {
                kind: "step_output".into(),
                from_step_run: Some(draft.clone()),
                from_item: None,
                label: "step:draft@1".into(),
                taint: Taint::Clean,
            }],
            taint: Taint::Clean,
            item: NewInboxItem {
                id: "item-g".into(),
                kind: InboxKind::Review,
                payload: Some(draft),
                taint: Taint::Clean,
                title: "Review before step approve goes on".into(),
                body: "Step `approve` waits for your review.".into(),
                max_returns: Some(2),
            },
        },
    )
    .unwrap();

    w.resolve_inbox_item(
        HumanActor::new("owner", REVIEWER_HAT),
        "item-g",
        Decision::Return,
        Some(NOTE_CANARY.to_string()),
    )
    .unwrap();
    w.exec(
        engine,
        WriteCommand::ApplyResolution {
            item_id: "item-g".into(),
            past_max: None,
        },
    )
    .unwrap();

    let (text, count, head) = export(&store);
    let entries: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let resolved: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|e| e["action"] == "inbox.resolved")
        .collect();
    assert_eq!(resolved.len(), 1, "{text}");
    let e = resolved[0];
    assert_eq!(
        e["actor"],
        serde_json::json!({"kind": "human", "person": "owner", "hat": "Reviewer"})
    );
    let actor: Actor = serde_json::from_value(e["actor"].clone()).unwrap();
    assert_eq!(actor.describe(), "You (Reviewer)");
    assert_eq!(e["target"], "item-g");
    assert_eq!(e["args"]["kind"], "review");
    assert_eq!(e["args"]["decision"], "return");
    assert_eq!(e["args"]["step_run_id"], approve.as_str());
    assert!(
        e["args"]["taint"] == serde_json::json!(0)
            || e["args"]["taint"] == serde_json::json!(false),
        "{e}"
    );
    assert_eq!(e["args"]["note_sha256"], sha256_hex(NOTE_CANARY));
    let at = entries
        .iter()
        .position(|x| x["action"] == "inbox.resolved")
        .unwrap();
    assert!(entries.len() > at + 1, "the apply is audited");
    for later in &entries[at + 1..] {
        assert_eq!(later["actor"]["kind"], "system", "{later}");
        assert_eq!(later["actor"]["component"], "engine", "{later}");
    }
    assert!(!text.contains(NOTE_CANARY), "the note leaked into:\n{text}");
    assert!(!text.contains("am-note-canary"), "{text}");
    let c = Connection::open(db(dir.path())).unwrap();
    let rows: Vec<String> = c
        .prepare("SELECT at || actor || action || target || outcome || coalesce(args, '') FROM audit_log")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for row in rows {
        assert!(!row.contains("am-note-canary"), "{row}");
    }
    let ok = verify_jsonl(text.as_bytes()).unwrap();
    assert_eq!((ok.entries, ok.head), (count, head));
}
