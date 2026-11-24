//! Story 3.1 (epic 3 story 1), the store's side of the one-step run (AD-6,
//! AD-8, AD-16): a run pins the config snapshot's hash in the transaction
//! that creates it, the writer stamps when a step started and when a step
//! and a run finished, the reader lists runs newest first, and migration
//! 0006 adds the four columns to an older database without touching its
//! rows. Written before the columns, the command field or the reader exist.
//!
//! What these tests pin:
//!
//! - `WriteCommand::CreateRun { .., config_hash: Option<String> }`: the hash
//!   of the `ConfigSnapshot` the run was built from (`None` only for a run
//!   with no snapshot, the spike's hard-coded workflow), audited as the
//!   `config_hash` argument of `run.created`.
//! - `RunRow { .., config_hash: Option<String>, created_at: String,
//!   finished_at: Option<String> }` and `StepRow { .., started_at:
//!   Option<String>, finished_at: Option<String> }`.
//! - `Reader::runs()`: every run, newest first.
//! - Migration `0006`: `runs.config_hash`, `runs.finished_at`,
//!   `step_runs.started_at`, `step_runs.finished_at`.
//!
//! The older tests' `CreateRun` literals are in `audit.rs` (with the
//! `run.created` arguments), `am-app/tests/audit_cli.rs` and
//! `am-e2e/tests/trust/rule_16_audit_log.rs`.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use am_domain::{
    Actor, Decision, HumanActor, InboxKind, REVIEWER_HAT, RuntimeLayout, Taint, Usage,
};
use am_store::{
    OpenReport, Reader, SCHEMA_VERSION, StepInput, Store, StoreError, WriteCommand, new_id,
};
use rusqlite::types::Value as Cell;
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};

/// The schema version before migration 0006.
const BEFORE_RUN_PIN: i64 = 5;

/// A config snapshot's hash: 64 lowercase hex characters.
const HASH_A: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";
const HASH_B: &str = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";

fn sys() -> Actor {
    Actor::System {
        component: "test".into(),
    }
}

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

/// The runtime folder the store opens: `rt/` in `root`.
fn layout(root: &Path) -> RuntimeLayout {
    RuntimeLayout::at(root.join("rt"))
}

fn db(root: &Path) -> PathBuf {
    layout(root).state_db()
}

fn open_project(root: &Path) -> Result<(Store, OpenReport), StoreError> {
    am_store::open_runtime(&layout(root), "test-mac".into())
}

/// The runtime folder made by hand, owner-only, as an earlier open left it.
fn existing_runtime(root: &Path) {
    let dir = layout(root).dir().to_path_buf();
    std::fs::create_dir(&dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}

fn create_run(store: &Store, actor: Actor, run_id: &str, hash: Option<&str>, steps: &[&str]) {
    store
        .writer()
        .exec(
            actor,
            WriteCommand::CreateRun {
                run_id: run_id.into(),
                workflow: "wf-1".into(),
                origin: "manual".into(),
                steps: steps.iter().map(|s| s.to_string()).collect(),
                config_hash: hash.map(str::to_string),
            },
        )
        .unwrap();
}

fn audit_entries(reader: &Reader) -> Vec<Value> {
    let mut out = Vec::new();
    reader.export_audit_jsonl(&mut out).unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn entry<'a>(all: &'a [Value], action: &str, target: &str) -> &'a Value {
    all.iter()
        .find(|e| e["action"] == action && e["target"] == target)
        .unwrap_or_else(|| panic!("no {action} entry for {target}"))
}

/// A store timestamp: UTC RFC 3339 with milliseconds and `Z`.
fn instant(text: &str) -> jiff::Timestamp {
    assert_eq!(text.len(), "2026-11-22T10:00:00.000Z".len(), "{text}");
    assert!(text.ends_with('Z'), "{text}");
    assert_eq!(text.as_bytes()[19], b'.', "milliseconds: {text}");
    text.parse()
        .unwrap_or_else(|e| panic!("{text} is not a timestamp: {e}"))
}

fn columns(db: &Path, table: &str) -> Vec<String> {
    let c = Connection::open(db).unwrap();
    let mut st = c
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap();
    st.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

// ------------------------------------------------------------------ the pin

/// Pin (AD-16): the run row holds the hash of the snapshot it was built
/// from, written by the command that creates the run, and the `run.created`
/// audit entry of that transaction carries it as an argument, by the Actor
/// who started the run.
#[test]
fn create_run_pins_the_config_hash_and_audits_it() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open_project(dir.path()).unwrap();
    create_run(&store, owner(), "run-1", Some(HASH_A), &["a"]);

    let reader = store.reader().unwrap();
    let run = reader.run("run-1").unwrap().expect("the run row");
    assert_eq!(run.status, "running");
    assert_eq!(run.workflow, "wf-1");
    assert_eq!(run.config_hash.as_deref(), Some(HASH_A));
    assert_eq!(run.finished_at, None);
    instant(&run.created_at);

    let all = audit_entries(&reader);
    let created = entry(&all, "run.created", "run-1");
    assert_eq!(
        created["args"],
        json!({"config_hash": HASH_A, "origin": "manual", "steps": 1, "workflow": "wf-1"})
    );
    assert_eq!(created["actor"]["kind"], "human");
    assert_eq!(created["actor"]["hat"], "Owner");
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
}

/// A run with no snapshot (the spike's hard-coded workflow) has no hash:
/// the column is NULL and the audit argument is `null`, never made up.
#[test]
fn a_run_without_a_snapshot_has_no_hash() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open_project(dir.path()).unwrap();
    create_run(&store, sys(), "run-1", None, &["a", "b"]);

    let reader = store.reader().unwrap();
    assert_eq!(reader.run("run-1").unwrap().unwrap().config_hash, None);
    let all = audit_entries(&reader);
    assert_eq!(
        entry(&all, "run.created", "run-1")["args"],
        json!({"config_hash": null, "origin": "manual", "steps": 2, "workflow": "wf-1"})
    );
}

// ------------------------------------------------------------------ times

/// The writer stamps, with its own clock: `started_at` when a step starts,
/// `finished_at` when it finishes (alone or with its call's settlement) and
/// the run's `finished_at` when the run finishes. A queued step has neither.
#[test]
fn the_writer_stamps_step_and_run_times() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open_project(dir.path()).unwrap();
    let w = store.writer();
    create_run(&store, owner(), "run-1", Some(HASH_A), &["a", "b", "c"]);
    let reader = store.reader().unwrap();
    let ids: Vec<String> = reader
        .steps("run-1")
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    for s in reader.steps("run-1").unwrap() {
        assert_eq!(s.status, "queued");
        assert_eq!(
            (s.started_at, s.finished_at),
            (None, None),
            "{}",
            s.step_key
        );
    }

    // Step a: started, then finished together with its call's settlement.
    w.exec(
        sys(),
        WriteCommand::StartStep {
            step_run_id: ids[0].clone(),
        },
    )
    .unwrap();
    let a = reader.steps("run-1").unwrap().remove(0);
    assert_eq!(a.status, "running");
    let a_started = instant(a.started_at.as_deref().expect("started_at"));
    assert_eq!(a.finished_at, None, "a running step has not finished");
    w.exec(
        sys(),
        WriteCommand::ReserveCall {
            step_run_id: ids[0].clone(),
            call_id: "call-1".into(),
            call_seq: 1,
            reserved_micro_usd: 0,
            rate_snapshot: "none".into(),
            // Story 3.3: raised, never lowered (AD-10).
            taint: Taint::Clean,
        },
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(5));
    w.exec(
        sys(),
        WriteCommand::SettleAndFinishStep {
            call_id: "call-1".into(),
            usage: Some(Usage {
                input_tokens: 12,
                output_tokens: 3,
            }),
            cost_micro_usd: None,
            step_run_id: ids[0].clone(),
            status: "succeeded".into(),
            output: Some("done".into()),
        },
    )
    .unwrap();
    let a = reader.steps("run-1").unwrap().remove(0);
    assert_eq!(a.status, "succeeded");
    assert_eq!(a.output.as_deref(), Some("done"));
    assert_eq!(
        instant(a.started_at.as_deref().unwrap()),
        a_started,
        "finishing never moves started_at"
    );
    let a_finished = instant(a.finished_at.as_deref().expect("finished_at"));
    assert!(a_finished > a_started, "{a_started} then {a_finished}");

    // Step b: started, then finished by the plain command.
    w.exec(
        sys(),
        WriteCommand::StartStep {
            step_run_id: ids[1].clone(),
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::FinishStep {
            step_run_id: ids[1].clone(),
            status: "failed".into(),
            output: None,
        },
    )
    .unwrap();
    let steps = reader.steps("run-1").unwrap();
    let b = &steps[1];
    assert_eq!(b.status, "failed");
    let b_started = instant(b.started_at.as_deref().expect("started_at"));
    let b_finished = instant(b.finished_at.as_deref().expect("finished_at"));
    assert!(b_started >= a_finished && b_finished >= b_started);
    // Step c never ran.
    assert_eq!(steps[2].status, "queued");
    assert_eq!(
        (steps[2].started_at.clone(), steps[2].finished_at.clone()),
        (None, None)
    );

    // The run.
    assert_eq!(reader.run("run-1").unwrap().unwrap().finished_at, None);
    w.exec(
        sys(),
        WriteCommand::FinishRun {
            run_id: "run-1".into(),
            status: "failed".into(),
        },
    )
    .unwrap();
    let run = reader.run("run-1").unwrap().unwrap();
    assert_eq!(run.status, "failed");
    let finished = instant(run.finished_at.as_deref().expect("finished_at"));
    assert!(finished >= b_finished);
    assert!(finished > instant(&run.created_at));
    assert_eq!(run.config_hash.as_deref(), Some(HASH_A), "the pin stays");

    // The commands kept their audit actions, in order.
    let actions: Vec<String> = audit_entries(&reader)
        .iter()
        .map(|e| e["action"].as_str().unwrap().to_string())
        .filter(|a| a != "store.migrated")
        .collect();
    assert_eq!(
        actions,
        vec![
            "run.created",
            "step.started",
            "model_call.reserved",
            "model_call.settled+step.finished",
            "step.started",
            "step.finished",
            "run.finished",
        ]
    );
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
}

// ------------------------------------------------------------------ reader

/// `Reader::runs()` lists every run, newest first, each with its workflow,
/// status, pinned hash and times; a database with no run lists none.
#[test]
fn runs_are_listed_newest_first_with_hash_and_times() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open_project(dir.path()).unwrap();
    let reader = store.reader().unwrap();
    assert!(reader.runs().unwrap().is_empty());

    let (first, second, third) = (new_id(), new_id(), new_id());
    create_run(&store, owner(), &first, Some(HASH_A), &["a"]);
    store
        .writer()
        .exec(
            sys(),
            WriteCommand::FinishRun {
                run_id: first.clone(),
                status: "succeeded".into(),
            },
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(5));
    create_run(&store, owner(), &second, Some(HASH_B), &["a"]);
    std::thread::sleep(Duration::from_millis(5));
    create_run(&store, sys(), &third, None, &["a"]);

    let runs = reader.runs().unwrap();
    let ids: Vec<&str> = runs.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec![third.as_str(), second.as_str(), first.as_str()]);
    assert_eq!(
        runs.iter()
            .map(|r| r.config_hash.as_deref())
            .collect::<Vec<_>>(),
        vec![None, Some(HASH_B), Some(HASH_A)]
    );
    assert_eq!(
        runs.iter().map(|r| r.status.as_str()).collect::<Vec<_>>(),
        vec!["running", "running", "succeeded"]
    );
    assert!(runs.iter().all(|r| r.workflow == "wf-1"));
    assert!(runs[2].finished_at.is_some());
    assert_eq!(runs[0].finished_at, None);
    assert!(instant(&runs[0].created_at) > instant(&runs[2].created_at));
    // The one-run read gives the same row.
    assert_eq!(reader.run(&second).unwrap().as_ref(), Some(&runs[1]));
}

// ------------------------------------------------------------------ migration

/// Migration 0006 on a schema-5 database that already holds a run: the four
/// columns are added, the old rows are kept with no hash and no times (a
/// run from before the pin never gets one made up), the step is reported
/// and audited like every migration, and the new commands then work on it.
#[test]
fn migration_0006_adds_the_pin_and_the_times_to_an_older_database() {
    let dir = tempfile::tempdir().unwrap();
    existing_runtime(dir.path());
    let path = db(dir.path());
    am_store::testing::database_at_version(&path, BEFORE_RUN_PIN).unwrap();
    assert!(!columns(&path, "runs").contains(&"config_hash".to_string()));
    {
        let c = Connection::open(&path).unwrap();
        c.execute(
            "INSERT INTO runs (id, workflow, origin, status, created_at, updated_at) VALUES ('old-run', 'wf-0', 'manual', 'succeeded', ?1, ?2)",
            params!["2020-01-02T10:00:00.000Z", "2020-01-02T10:00:04.000Z"],
        )
        .unwrap();
        c.execute(
            "INSERT INTO step_runs (id, run_id, step_key, seq, status, output, updated_at) VALUES ('old-step', 'old-run', 'a', 0, 'succeeded', 'kept', ?1)",
            params!["2020-01-02T10:00:04.000Z"],
        )
        .unwrap();
    }

    let (store, report) = open_project(dir.path()).unwrap();

    assert_eq!(report.version, SCHEMA_VERSION);
    assert!(!report.created);
    let step = report
        .applied
        .iter()
        .find(|m| m.from == BEFORE_RUN_PIN)
        .expect("the 5 to 6 step is reported");
    assert_eq!(step.to, BEFORE_RUN_PIN + 1);
    assert!(
        step.backup.as_ref().is_some_and(|b| b.is_file()),
        "a backup was taken first: {step:?}"
    );
    for column in ["config_hash", "finished_at"] {
        assert!(
            columns(&path, "runs").contains(&column.to_string()),
            "runs.{column}"
        );
    }
    for column in ["started_at", "finished_at"] {
        assert!(
            columns(&path, "step_runs").contains(&column.to_string()),
            "step_runs.{column}"
        );
    }

    // The old rows: kept, with nothing made up.
    let reader = store.reader().unwrap();
    let old = reader.run("old-run").unwrap().expect("the old run");
    assert_eq!(
        (old.workflow.as_str(), old.status.as_str()),
        ("wf-0", "succeeded")
    );
    assert_eq!(old.created_at, "2020-01-02T10:00:00.000Z");
    assert_eq!((old.config_hash, old.finished_at), (None, None));
    let old_steps = reader.steps("old-run").unwrap();
    assert_eq!(old_steps.len(), 1);
    assert_eq!(old_steps[0].output.as_deref(), Some("kept"));
    assert_eq!(
        (
            old_steps[0].started_at.clone(),
            old_steps[0].finished_at.clone()
        ),
        (None, None)
    );

    // The new commands work on the migrated database, and the old run is
    // listed after the new one.
    let new_run = new_id();
    create_run(&store, owner(), &new_run, Some(HASH_A), &["a"]);
    let step_id = reader.steps(&new_run).unwrap()[0].id.clone();
    store
        .writer()
        .exec(
            sys(),
            WriteCommand::StartStep {
                step_run_id: step_id,
            },
        )
        .unwrap();
    assert!(reader.steps(&new_run).unwrap()[0].started_at.is_some());
    let ids: Vec<String> = reader.runs().unwrap().into_iter().map(|r| r.id).collect();
    assert_eq!(ids, vec![new_run, "old-run".to_string()]);

    // The step is audited like every migration, and the chain verifies.
    let all = audit_entries(&reader);
    assert!(
        all.iter().any(|e| e["action"] == "store.migrated"
            && e["outcome"]
                .as_str()
                .is_some_and(|o| o.starts_with(&format!(
                    "from version {BEFORE_RUN_PIN} to {}",
                    BEFORE_RUN_PIN + 1
                )))),
        "{all:#?}"
    );
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
}

// ------------------------------------------------------------------ story 3.3
//
// Epic 3 story 3 (AD-7, AD-8): every step attempt is a step-run row with an
// `attempt` number and a `taint` flag; migration 0007 rebuilds `step_runs`
// (its `UNIQUE (run_id, step_key)` becomes `UNIQUE (run_id, step_key,
// attempt)`) and adds `step_inputs`, `tool_calls` and `inbox_items`; a
// loop-back closes the producing attempt `looped_back` and queues new
// attempts from the target step through the producer, in one transaction,
// never rewriting an old row. Written before migration 0007 and the
// commands exist.

/// The schema version before migration 0007.
const BEFORE_ATTEMPTS: i64 = 6;

/// Row 29, Old database: a schema-6 database holding a finished run and an
/// unfinished one (a settled call, a queued step) migrates to 0007 after a
/// backup; every row is kept as attempt 1, untainted, with its output,
/// status and times; the new tables are empty; foreign keys still hold;
/// the step is reported and audited; and the new commands work on it.
#[test]
fn migration_0007_keeps_every_row_as_attempt_one_untainted() {
    let dir = tempfile::tempdir().unwrap();
    existing_runtime(dir.path());
    let path = db(dir.path());
    am_store::testing::database_at_version(&path, BEFORE_ATTEMPTS).unwrap();
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
               VALUES ('old-run', 'wf-0', 'manual', 'running', '2026-01-02T10:00:00.000Z', '2026-01-02T10:00:04.000Z', NULL, NULL);
             INSERT INTO step_runs (id, run_id, step_key, seq, status, output, updated_at, started_at, finished_at)
               VALUES ('old-a', 'old-run', 'a', 0, 'succeeded', 'kept output', '2026-01-02T10:00:03.000Z', '2026-01-02T10:00:01.000Z', '2026-01-02T10:00:03.000Z');
             INSERT INTO step_runs (id, run_id, step_key, seq, status, output, updated_at, started_at, finished_at)
               VALUES ('old-b', 'old-run', 'b', 1, 'queued', NULL, '2026-01-02T10:00:00.000Z', NULL, NULL);
             INSERT INTO model_calls (id, step_run_id, call_seq, status, settled, input_tokens, output_tokens, created_at)
               VALUES ('old-call', 'old-a', 1, 'settled', 1, 12, 3, '2026-01-02T10:00:02.000Z');
             INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at)
               VALUES ('old-ledger', 'old-call', 'old-run', 'model', 0, 0, 'settled', 'none', '2026-01-02T10:00:02.000Z');",
        )
        .unwrap();
    }

    let (store, report) = open_project(dir.path()).unwrap();

    // Story 3.4: 0008 follows 0007, so the 6 → 7 step is applied (and
    // reported, below) but is no longer the last.
    const {
        assert!(
            SCHEMA_VERSION > BEFORE_ATTEMPTS,
            "the 6 → 7 step is applied"
        )
    };
    assert_eq!(report.version, SCHEMA_VERSION);
    let step = report
        .applied
        .iter()
        .find(|m| m.from == BEFORE_ATTEMPTS)
        .expect("the 6 to 7 step is reported");
    assert_eq!(step.to, BEFORE_ATTEMPTS + 1);
    assert!(
        step.backup.as_ref().is_some_and(|b| b.is_file()),
        "a pre-migration copy first (AD-7): {step:?}"
    );
    for column in ["attempt", "taint"] {
        assert!(
            columns(&path, "step_runs").contains(&column.to_string()),
            "step_runs.{column}"
        );
    }
    for table in ["step_inputs", "tool_calls", "inbox_items"] {
        assert!(!columns(&path, table).is_empty(), "the table {table}");
    }

    let reader = store.reader().unwrap();
    let steps = reader.steps("old-run").unwrap();
    assert_eq!(
        steps
            .iter()
            .map(|s| (
                s.id.as_str(),
                s.step_key.as_str(),
                s.seq,
                s.attempt,
                s.status.as_str(),
                s.output.as_deref(),
                s.taint,
            ))
            .collect::<Vec<_>>(),
        vec![
            (
                "old-a",
                "a",
                0,
                1,
                "succeeded",
                Some("kept output"),
                Taint::Clean
            ),
            ("old-b", "b", 1, 1, "queued", None, Taint::Clean),
        ]
    );
    assert_eq!(
        (
            steps[0].started_at.as_deref(),
            steps[0].finished_at.as_deref()
        ),
        (
            Some("2026-01-02T10:00:01.000Z"),
            Some("2026-01-02T10:00:03.000Z")
        )
    );
    for s in &steps {
        assert!(reader.step_inputs(&s.id).unwrap().is_empty());
        assert!(reader.tool_calls(&s.id).unwrap().is_empty());
    }
    assert!(reader.inbox_items("old-run").unwrap().is_empty());
    let calls = reader.calls_for_run("old-run").unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].step_run_id.as_str(), calls[0].input_tokens),
        ("old-a", Some(12))
    );
    assert_eq!(reader.ledger_for_run("old-run").unwrap().len(), 1);
    let broken: i64 = Connection::open(&path)
        .unwrap()
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(broken, 0, "the rebuild kept every reference");

    // The new commands work on the old run: its queued step starts.
    store
        .writer()
        .exec(
            sys(),
            WriteCommand::StartAttempt {
                step_run_id: "old-b".into(),
                inputs: vec![],
                taint: Taint::Clean,
            },
        )
        .unwrap();
    assert_eq!(reader.steps("old-run").unwrap()[1].status, "running");

    let all = audit_entries(&reader);
    assert!(
        all.iter().any(|e| e["action"] == "store.migrated"
            && e["outcome"]
                .as_str()
                .is_some_and(|o| o.starts_with(&format!(
                    "from version {BEFORE_ATTEMPTS} to {}",
                    BEFORE_ATTEMPTS + 1
                )))),
        "{all:#?}"
    );
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
}

/// Starts step `id` (clean, no inputs), reserves its call `call` and
/// settles it with the step's finish `succeeded`, holding `output`.
fn run_step(store: &Store, id: &str, call: &str, output: &str) {
    let w = store.writer();
    w.exec(
        sys(),
        WriteCommand::StartAttempt {
            step_run_id: id.into(),
            inputs: vec![],
            taint: Taint::Clean,
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::ReserveCall {
            step_run_id: id.into(),
            call_id: call.into(),
            call_seq: 1,
            reserved_micro_usd: 0,
            rate_snapshot: "none".into(),
            taint: Taint::Clean,
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::SettleAndFinishStep {
            call_id: call.into(),
            usage: None,
            cost_micro_usd: None,
            step_run_id: id.into(),
            status: "succeeded".into(),
            output: Some(output.into()),
        },
    )
    .unwrap();
}

/// Row 8 at the store: a loop-back from `check` to `draft` settles the
/// producing call and closes `check`'s attempt 1 `looped_back` with its
/// output stored, and queues attempt 2 of `draft`, `edit` and `check` (not
/// of `publish`, after the producer), in one transaction; attempt-1 rows
/// are never rewritten; the reader lists every attempt in workflow order.
/// A target that is unknown or after the producer is refused and changes
/// nothing.
#[test]
fn a_loop_back_keeps_the_old_attempt_and_queues_new_ones() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open_project(dir.path()).unwrap();
    let w = store.writer();
    create_run(
        &store,
        owner(),
        "run-1",
        Some(HASH_A),
        &["draft", "edit", "check", "publish"],
    );
    let reader = store.reader().unwrap();
    let ids: Vec<String> = reader
        .steps("run-1")
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    run_step(&store, &ids[0], "call-1", "version one");
    run_step(&store, &ids[1], "call-2", "edited one");
    w.exec(
        sys(),
        WriteCommand::StartAttempt {
            step_run_id: ids[2].clone(),
            inputs: vec![],
            taint: Taint::Clean,
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::ReserveCall {
            step_run_id: ids[2].clone(),
            call_id: "call-3".into(),
            call_seq: 1,
            reserved_micro_usd: 0,
            rate_snapshot: "none".into(),
            taint: Taint::Clean,
        },
    )
    .unwrap();
    let before = reader.steps("run-1").unwrap();
    let audited = audit_entries(&reader).len();
    let reasons = r#"{"reasons":["tighten the opening"]}"#;

    // Refused: a target the run does not have, or one after the producer.
    for to in ["nowhere", "publish"] {
        let refused = w.exec(
            sys(),
            WriteCommand::SettleAndLoopBack {
                call_id: "call-3".into(),
                usage: None,
                cost_micro_usd: None,
                step_run_id: ids[2].clone(),
                output: reasons.into(),
                to: to.into(),
            },
        );
        assert!(refused.is_err(), "{to}");
        assert_eq!(reader.steps("run-1").unwrap(), before, "{to}");
        assert!(!reader.calls_for_run("run-1").unwrap()[2].settled, "{to}");
        assert_eq!(audit_entries(&reader).len(), audited, "{to}");
    }

    w.exec(
        sys(),
        WriteCommand::SettleAndLoopBack {
            call_id: "call-3".into(),
            usage: Some(Usage {
                input_tokens: 4,
                output_tokens: 2,
            }),
            cost_micro_usd: None,
            step_run_id: ids[2].clone(),
            output: reasons.into(),
            to: "draft".into(),
        },
    )
    .unwrap();

    let after = reader.steps("run-1").unwrap();
    assert_eq!(
        after
            .iter()
            .map(|s| (s.step_key.as_str(), s.seq, s.attempt, s.status.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("draft", 0, 1, "succeeded"),
            ("draft", 0, 2, "queued"),
            ("edit", 1, 1, "succeeded"),
            ("edit", 1, 2, "queued"),
            ("check", 2, 1, "looped_back"),
            ("check", 2, 2, "queued"),
            ("publish", 3, 1, "queued"),
        ]
    );
    let old = |key: &str, rows: &[am_store::StepRow]| {
        rows.iter()
            .find(|s| s.step_key == key && s.attempt == 1)
            .cloned()
            .unwrap()
    };
    for key in ["draft", "edit", "publish"] {
        assert_eq!(old(key, &after), old(key, &before), "{key} attempt 1");
    }
    let check1 = old("check", &after);
    assert_eq!(check1.output.as_deref(), Some(reasons));
    assert!(check1.finished_at.is_some());
    assert_eq!(check1.started_at, old("check", &before).started_at);
    for s in after.iter().filter(|s| s.attempt == 2) {
        assert!(!ids.contains(&s.id), "a new row: {s:?}");
        assert_eq!((s.started_at.clone(), s.finished_at.clone()), (None, None));
        assert_eq!(s.output, None);
        assert_eq!(s.taint, Taint::Clean);
    }
    let call = reader.calls_for_run("run-1").unwrap()[2].clone();
    assert!(call.settled);
    assert_eq!((call.input_tokens, call.output_tokens), (Some(4), Some(2)));

    // Audited once, with ids and counts only.
    let all = audit_entries(&reader);
    let looped: Vec<&Value> = all
        .iter()
        .filter(|e| {
            e["action"]
                .as_str()
                .is_some_and(|a| a.contains("step.looped_back"))
        })
        .collect();
    assert_eq!(looped.len(), 1, "{all:#?}");
    assert!(!looped[0].to_string().contains("tighten the opening"));
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
}

// ------------------------------------------------------- review round 1
//
// Story 3.3 review patch round 1 (coordinator's triage, 2026-11-23): P9
// (the loop-back edge is stored once, as the target's `loop_back` input,
// AD-43) and P10 (a migration step fails only on a reference it broke).
// Written before the patches.

/// P9: the journal holds the loop-back edge once: `step_runs` has no
/// `loop_back_from` column, and the loop-back's own transaction writes the
/// target's new attempt's `loop_back` input (from the producing attempt,
/// labelled `loop-back:<key>@<attempt>`, with the producer's taint), while
/// that attempt is still `queued`. The other new attempts have no input
/// yet (theirs come when they start).
#[test]
fn the_loop_back_input_is_journaled_by_the_loop_back() {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open_project(dir.path()).unwrap();
    create_run(
        &store,
        owner(),
        "run-1",
        Some(HASH_A),
        &["draft", "edit", "check"],
    );
    assert!(
        !columns(&db(dir.path()), "step_runs").contains(&"loop_back_from".to_string()),
        "the edge is not stored twice (AD-43)"
    );
    let reader = store.reader().unwrap();
    let ids: Vec<String> = reader
        .steps("run-1")
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    run_step(&store, &ids[0], "call-1", "version one");
    run_step(&store, &ids[1], "call-2", "edited one");
    let w = store.writer();
    w.exec(
        sys(),
        WriteCommand::StartAttempt {
            step_run_id: ids[2].clone(),
            inputs: vec![],
            taint: Taint::Tainted,
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::ReserveCall {
            step_run_id: ids[2].clone(),
            call_id: "call-3".into(),
            call_seq: 1,
            reserved_micro_usd: 0,
            rate_snapshot: "none".into(),
            taint: Taint::Tainted,
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::SettleAndLoopBack {
            call_id: "call-3".into(),
            usage: None,
            cost_micro_usd: None,
            step_run_id: ids[2].clone(),
            output: r#"{"reasons":["tighten the opening"]}"#.into(),
            to: "draft".into(),
        },
    )
    .unwrap();

    let steps = reader.steps("run-1").unwrap();
    let attempt2 = |key: &str| {
        steps
            .iter()
            .find(|s| s.step_key == key && s.attempt == 2)
            .cloned()
            .unwrap()
    };
    let draft2 = attempt2("draft");
    assert_eq!(draft2.status, "queued");
    assert_eq!(
        reader.step_inputs(&draft2.id).unwrap(),
        vec![StepInput {
            kind: "loop_back".into(),
            from_step_run: Some(ids[2].clone()),
            from_item: None,
            label: "loop-back:check@1".into(),
            taint: Taint::Tainted,
        }],
        "written by the loop-back, before the attempt starts"
    );
    for key in ["edit", "check"] {
        assert!(
            reader.step_inputs(&attempt2(key).id).unwrap().is_empty(),
            "{key}"
        );
    }
}

/// P10: a reference that was already broken before a migration step (a
/// row left by an older bug, or by hand, with foreign keys off) does not
/// fail the step: the step fails only on a reference it broke. The row is
/// kept as it was.
#[test]
fn a_reference_broken_before_the_migration_does_not_fail_it() {
    let dir = tempfile::tempdir().unwrap();
    existing_runtime(dir.path());
    let path = db(dir.path());
    am_store::testing::database_at_version(&path, BEFORE_ATTEMPTS).unwrap();
    {
        let c = Connection::open(&path).unwrap();
        c.pragma_update(None, "foreign_keys", "OFF").unwrap();
        c.execute_batch(
            "INSERT INTO runs (id, workflow, origin, status, created_at, updated_at)
               VALUES ('old-run', 'wf-0', 'manual', 'failed', '2026-01-02T10:00:00.000Z', '2026-01-02T10:00:04.000Z');
             INSERT INTO model_calls (id, step_run_id, call_seq, status, settled, created_at)
               VALUES ('orphan-call', 'no-such-step', 1, 'settled', 1, '2026-01-02T10:00:02.000Z');",
        )
        .unwrap();
        let broken: i64 = c
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(broken, 1, "the fixture holds one broken reference");
    }

    let opened = open_project(dir.path());

    let (store, report) = opened.expect("a reference broken before the step does not fail it");
    assert_eq!(report.version, SCHEMA_VERSION);
    let kept: i64 = Connection::open(&path)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM model_calls WHERE id = 'orphan-call'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, 1, "the row is kept as it was");
    assert_eq!(
        store
            .reader()
            .unwrap()
            .run("old-run")
            .unwrap()
            .unwrap()
            .status,
        "failed"
    );
}

// ------------------------------------------------------------------ story 3.4
//
// Epic 3 story 4 (AD-7, AD-28): migration 0008 rebuilds `inbox_items` so an
// item can be resolved: `status` takes `open` or `resolved`, the new
// columns `max_returns` (a Review's), `decision` and `note` join
// `resolved_by` and `resolved_at`, a table CHECK ties them to the status,
// 0007's taint trigger is made again beside two new ones (a resolved row
// never changes; an item's own fields never change), an index on
// `(status, created_at)` is added, and `step_inputs` gains `from_item`. It
// runs behind AD-7's pre-migration copy; every item keeps every 0007 column
// and stays `open`, a Review taking `max_returns` 2 (3.3's gates had the
// default). Written before migration 0008 exists.

/// The schema version before migration 0008.
const BEFORE_RESOLUTION: i64 = 7;

/// Every column `inbox_items` has at version 7.
const ITEM_COLUMNS_0007: &str = "id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, resolved_at, resolved_by";

/// The columns of a step run at version 7.
const STEP_COLUMNS_0007: &str = "id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at";

/// A schema-7 database in the project's runtime folder, made by hand.
fn database_at_7(root: &Path) -> PathBuf {
    existing_runtime(root);
    let path = db(root);
    am_store::testing::database_at_version(&path, BEFORE_RESOLUTION).unwrap();
    path
}

/// Row 37's schema-7 rows: run `old-run` waits at its gate `approve` with
/// an open, tainted Review item whose payload is `draft`'s tainted
/// attempt, and the gate's journaled `step_output` input; run `old-fail`
/// failed at its step `work` with an open, clean Failure item.
fn row_37(path: &Path) {
    Connection::open(path)
        .unwrap()
        .execute_batch(&format!(
            "INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
               VALUES ('old-run', 'wf-gated', 'manual', 'running', '2026-11-20T10:00:00.000Z', '2026-11-20T10:00:04.000Z', NULL, NULL);
             INSERT INTO step_runs ({STEP_COLUMNS_0007})
               VALUES ('old-draft', 'old-run', 'draft', 0, 1, 'succeeded', 'the old version', 1, '2026-11-20T10:00:03.000Z', '2026-11-20T10:00:01.000Z', '2026-11-20T10:00:03.000Z');
             INSERT INTO step_runs ({STEP_COLUMNS_0007})
               VALUES ('old-approve', 'old-run', 'approve', 1, 1, 'waiting_human', NULL, 1, '2026-11-20T10:00:04.000Z', '2026-11-20T10:00:04.000Z', NULL);
             INSERT INTO step_runs ({STEP_COLUMNS_0007})
               VALUES ('old-publish', 'old-run', 'publish', 2, 1, 'queued', NULL, 0, '2026-11-20T10:00:00.000Z', NULL, NULL);
             INSERT INTO step_inputs (step_run_id, kind, from_step_run, label, taint, created_at)
               VALUES ('old-approve', 'step_output', 'old-draft', 'step:draft@1', 1, '2026-11-20T10:00:04.000Z');
             INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at)
               VALUES ('old-review', 'review', 'open', 'old-run', 'old-approve', 'old-draft', 1, 'Review before step approve goes on', 'Step `approve` waits for your review.', '2026-11-20T10:00:04.000Z');
             INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
               VALUES ('old-fail', 'wf-capped', 'manual', 'failed', '2026-11-20T11:00:00.000Z', '2026-11-20T11:00:09.000Z', NULL, '2026-11-20T11:00:09.000Z');
             INSERT INTO step_runs ({STEP_COLUMNS_0007})
               VALUES ('old-work', 'old-fail', 'work', 0, 1, 'failed', NULL, 0, '2026-11-20T11:00:09.000Z', '2026-11-20T11:00:01.000Z', '2026-11-20T11:00:09.000Z');
             INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at)
               VALUES ('old-failure', 'failure', 'open', 'old-fail', 'old-work', 'old-work', 0, 'A step stopped', 'Step `work` stopped at its limit of 3 iterations.', '2026-11-20T11:00:09.000Z');"
        ))
        .unwrap();
}

/// Every row of `inbox_items` (by id) as the raw cells of `columns`.
fn item_cells(conn: &Connection, columns: &str) -> Vec<Vec<Cell>> {
    let mut st = conn
        .prepare(&format!("SELECT {columns} FROM inbox_items ORDER BY id"))
        .unwrap();
    let n = st.column_count();
    st.query_map([], |r| {
        (0..n)
            .map(|i| r.get::<_, Cell>(i))
            .collect::<rusqlite::Result<Vec<Cell>>>()
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn user_version(conn: &Connection) -> i64 {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

/// The names of the triggers on `inbox_items`.
fn item_triggers(path: &Path) -> Vec<String> {
    Connection::open(path)
        .unwrap()
        .prepare("SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'inbox_items' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// The owner as the Reviewer: the one resolver (AD-15).
fn reviewer() -> HumanActor {
    HumanActor::new("owner", REVIEWER_HAT)
}

/// Row 37: a schema-7 database with an open, tainted Review (a waiting
/// gate with its journaled input) and an open Failure migrates to 0008
/// after a pre-migration copy; the step is reported 7 → 8; the new columns,
/// `step_inputs.from_item` and the index exist; every item keeps every
/// 0007 column value for value, stays `open` with no resolution, keeps its
/// taint, and a Review takes `max_returns` 2; the input row is kept with no
/// item; `UNIQUE (step_run_id, kind)` and the foreign keys hold; the new
/// commands then resolve and apply the old Review; `store.migrated` is
/// audited 7 → 8 and the chain verifies.
#[test]
fn migration_0008_keeps_every_item_open_with_its_taint() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_7(dir.path());
    row_37(&path);
    let before = item_cells(&Connection::open(&path).unwrap(), ITEM_COLUMNS_0007);

    let (store, report) = open_project(dir.path()).unwrap();

    assert_eq!(
        SCHEMA_VERSION,
        BEFORE_RESOLUTION + 1,
        "0008 is this story's"
    );
    assert_eq!(report.version, SCHEMA_VERSION);
    let step = report
        .applied
        .iter()
        .find(|m| m.from == BEFORE_RESOLUTION)
        .expect("the 7 to 8 step is reported");
    assert_eq!(step.to, BEFORE_RESOLUTION + 1);
    assert!(
        step.backup.as_ref().is_some_and(|b| b.is_file()),
        "a pre-migration copy first (AD-7): {step:?}"
    );
    assert!(
        report
            .lines()
            .iter()
            .any(|l| l.contains("from version 7 to 8")),
        "{:?}",
        report.lines()
    );
    for column in [
        "max_returns",
        "decision",
        "note",
        "resolved_by",
        "resolved_at",
    ] {
        assert!(
            columns(&path, "inbox_items").contains(&column.to_string()),
            "inbox_items.{column}"
        );
    }
    assert!(columns(&path, "step_inputs").contains(&"from_item".to_string()));
    let c = Connection::open(&path).unwrap();
    let index: i64 = c
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'inbox_items_by_status' AND tbl_name = 'inbox_items'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(index, 1, "the (status, created_at) index");

    // Every item: every 0007 column, value for value.
    assert_eq!(item_cells(&c, ITEM_COLUMNS_0007), before);
    let reader = store.reader().unwrap();
    let review = reader.inbox_item("old-review").unwrap().expect("kept");
    assert_eq!(
        (review.kind, review.status.as_str(), review.taint),
        (InboxKind::Review, "open", Taint::Tainted)
    );
    assert_eq!(
        (
            review.decision,
            review.note.clone(),
            review.resolved_by.clone(),
            review.resolved_at.clone()
        ),
        (None, None, None, None)
    );
    assert_eq!(review.max_returns, Some(2), "a 3.3 gate had the default");
    assert_eq!(review.payload.as_deref(), Some("old-draft"));
    let failure = reader.inbox_item("old-failure").unwrap().expect("kept");
    assert_eq!(
        (failure.kind, failure.status.as_str(), failure.taint),
        (InboxKind::Failure, "open", Taint::Clean)
    );
    assert_eq!(failure.max_returns, None);
    let mut open: Vec<String> = reader
        .open_items()
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .collect();
    open.sort();
    assert_eq!(
        open,
        vec!["old-failure".to_string(), "old-review".to_string()]
    );
    assert_eq!(
        reader.step_inputs("old-approve").unwrap(),
        vec![StepInput {
            kind: "step_output".into(),
            from_step_run: Some("old-draft".into()),
            from_item: None,
            label: "step:draft@1".into(),
            taint: Taint::Tainted,
        }]
    );
    let taints: Vec<(String, Taint)> = reader
        .steps("old-run")
        .unwrap()
        .into_iter()
        .map(|s| (s.step_key, s.taint))
        .collect();
    assert_eq!(
        taints,
        vec![
            ("draft".to_string(), Taint::Tainted),
            ("approve".to_string(), Taint::Tainted),
            ("publish".to_string(), Taint::Clean),
        ]
    );
    let broken: i64 = c
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(broken, 0, "the rebuild kept every reference");
    assert!(
        c.execute(
            "INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at)
             VALUES ('dup', 'review', 'open', 'old-run', 'old-approve', NULL, 1, 't', 'b', '2026-11-20T10:00:06.000Z')",
            [],
        )
        .is_err(),
        "UNIQUE (step_run_id, kind) holds"
    );
    drop(c);

    // The new commands work on the migrated database.
    store
        .writer()
        .resolve_inbox_item(reviewer(), "old-review", Decision::Approve, None)
        .unwrap();
    store
        .writer()
        .exec(
            sys(),
            WriteCommand::ApplyResolution {
                item_id: "old-review".into(),
                past_max: None,
            },
        )
        .unwrap();
    let approve = reader
        .steps("old-run")
        .unwrap()
        .into_iter()
        .find(|s| s.step_key == "approve")
        .unwrap();
    assert_eq!(approve.status, "succeeded");
    let review = reader.inbox_item("old-review").unwrap().unwrap();
    assert_eq!(review.status, "resolved");
    assert_eq!(review.taint, Taint::Tainted, "resolving keeps the taint");

    let all = audit_entries(&reader);
    assert!(
        all.iter().any(|e| e["action"] == "store.migrated"
            && e["outcome"]
                .as_str()
                .is_some_and(|o| o.starts_with(&format!(
                    "from version {BEFORE_RESOLUTION} to {}",
                    BEFORE_RESOLUTION + 1
                )))),
        "{all:#?}"
    );
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
}

/// Row 37: after 0008, `inbox_items` has 0007's taint trigger again and
/// its two new triggers. A lower taint is aborted (a higher one is not);
/// an `UPDATE` of an item's own fields (id, kind, run, step run, payload,
/// title, body, creation time, `max_returns`) is aborted; the table CHECK
/// refuses a status but `open` or `resolved`, an open row with a
/// resolution, a resolved row without its decision, resolver or time, and
/// a `max_returns` outside 0..=5; once resolved, any `UPDATE` of the row
/// is aborted.
#[test]
fn migration_0008_keeps_the_taint_trigger_and_adds_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_7(dir.path());
    row_37(&path);
    let (store, _) = open_project(dir.path()).unwrap();

    let triggers = item_triggers(&path);
    for name in [
        "inbox_items_taint_only_rises",
        "inbox_items_resolved_never_change",
        "inbox_items_fixed_fields",
    ] {
        assert!(triggers.contains(&name.to_string()), "{name}: {triggers:?}");
    }

    let c = Connection::open(&path).unwrap();
    assert!(
        c.execute(
            "UPDATE inbox_items SET taint = 0 WHERE id = 'old-review'",
            []
        )
        .is_err(),
        "taint only rises"
    );
    for (column, value) in [
        ("id", "'another-id'"),
        ("kind", "'question'"),
        ("run_id", "'old-fail'"),
        ("step_run_id", "'old-draft'"),
        ("payload", "NULL"),
        ("title", "'another title'"),
        ("body", "'another body'"),
        ("created_at", "'2020-01-01T00:00:00.000Z'"),
        ("max_returns", "3"),
    ] {
        assert!(
            c.execute(
                &format!("UPDATE inbox_items SET {column} = {value} WHERE id = 'old-review'"),
                []
            )
            .is_err(),
            "{column} never changes"
        );
    }
    let insert = |status: &str, decision: &str, by: &str, at: &str, max: &str| {
        c.execute(
            &format!(
                "INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, max_returns, decision, note, resolved_by, resolved_at)
                 VALUES ('{}', 'question', '{status}', 'old-run', 'old-draft', NULL, 0, 't', 'b', '2026-11-20T10:00:06.000Z', {max}, {decision}, NULL, {by}, {at})",
                new_id()
            ),
            [],
        )
    };
    let human = "'{\"kind\":\"human\",\"person\":\"owner\",\"hat\":\"Reviewer\"}'";
    let when = "'2026-11-20T10:00:07.000Z'";
    for (what, status, decision, by, at, max) in [
        (
            "a status but open or resolved",
            "closed",
            "NULL",
            "NULL",
            "NULL",
            "NULL",
        ),
        (
            "an open row with a decision",
            "open",
            "'answer'",
            "NULL",
            "NULL",
            "NULL",
        ),
        (
            "an open row with a resolver",
            "open",
            "NULL",
            human,
            when,
            "NULL",
        ),
        (
            "a resolved row without a decision",
            "resolved",
            "NULL",
            human,
            when,
            "NULL",
        ),
        (
            "a resolved row without a resolver",
            "resolved",
            "'answer'",
            "NULL",
            when,
            "NULL",
        ),
        (
            "a resolved row without a time",
            "resolved",
            "'answer'",
            human,
            "NULL",
            "NULL",
        ),
        ("max_returns 6", "open", "NULL", "NULL", "NULL", "6"),
        ("max_returns -1", "open", "NULL", "NULL", "NULL", "-1"),
    ] {
        assert!(
            insert(status, decision, by, at, max).is_err(),
            "the CHECK refuses {what}"
        );
    }
    assert!(
        c.execute(
            "UPDATE inbox_items SET taint = 1 WHERE id = 'old-failure'",
            []
        )
        .is_ok(),
        "a taint may rise on an open item"
    );
    drop(c);

    store
        .writer()
        .resolve_inbox_item(reviewer(), "old-failure", Decision::Dismiss, None)
        .unwrap();
    let c = Connection::open(&path).unwrap();
    for sql in [
        "UPDATE inbox_items SET note = 'x' WHERE id = 'old-failure'",
        "UPDATE inbox_items SET decision = 'approve' WHERE id = 'old-failure'",
        "UPDATE inbox_items SET status = 'open', decision = NULL, resolved_by = NULL, resolved_at = NULL WHERE id = 'old-failure'",
    ] {
        assert!(
            c.execute(sql, []).is_err(),
            "a resolved row never changes: {sql}"
        );
    }
}

/// Row 37: the first open migrates 7 → 8; reopening the migrated store
/// applies nothing (no step, no line, no new `store.migrated` entry) and
/// every item is as it was.
#[test]
fn reopening_a_migrated_store_applies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_7(dir.path());
    row_37(&path);
    let (store, first) = open_project(dir.path()).unwrap();
    assert!(
        first.applied.iter().any(|m| m.from == BEFORE_RESOLUTION),
        "the first open migrates 7 to 8: {first:?}"
    );
    let reader = store.reader().unwrap();
    let items = (
        reader.inbox_items("old-run").unwrap(),
        reader.inbox_items("old-fail").unwrap(),
    );
    let migrated = audit_entries(&reader)
        .iter()
        .filter(|e| e["action"] == "store.migrated")
        .count();
    drop(reader);
    drop(store);

    let (store, again) = open_project(dir.path()).unwrap();

    assert!(again.applied.is_empty(), "{again:?}");
    assert!(again.lines().is_empty(), "{:?}", again.lines());
    assert_eq!(again.version, SCHEMA_VERSION);
    let reader = store.reader().unwrap();
    assert_eq!(
        (
            reader.inbox_items("old-run").unwrap(),
            reader.inbox_items("old-fail").unwrap()
        ),
        items
    );
    assert_eq!(
        audit_entries(&reader)
            .iter()
            .filter(|e| e["action"] == "store.migrated")
            .count(),
        migrated
    );
}

/// Row 37: a forged schema-7 row (an `open` item carrying `resolved_by`
/// and `resolved_at`, which no 3.3 command wrote) fails the 7 → 8 step
/// closed: the open is refused naming the backup, the database stays at
/// version 7 with every row as it was, the pre-migration copy is kept, and
/// a later open retries the step (and fails the same way).
#[test]
fn a_forged_resolution_fails_0008_closed() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_7(dir.path());
    row_37(&path);
    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE inbox_items SET resolved_by = '{\"kind\":\"system\",\"component\":\"engine\"}', resolved_at = '2026-11-20T10:00:05.000Z' WHERE id = 'old-review'",
            [],
        )
        .unwrap();
    let before = item_cells(&Connection::open(&path).unwrap(), ITEM_COLUMNS_0007);

    let refused = match open_project(dir.path()) {
        Ok((_, report)) => panic!("the forged row must fail 0008 closed: {report:?}"),
        Err(e) => e,
    };

    let words = refused.to_string();
    match &refused {
        StoreError::MigrationFailed {
            from, to, backup, ..
        } => {
            assert_eq!((*from, *to), (BEFORE_RESOLUTION, BEFORE_RESOLUTION + 1));
            let backup = backup.as_ref().expect("the step names its backup");
            assert!(backup.is_file(), "the pre-migration copy is kept");
            assert!(
                words.contains(&backup.display().to_string()),
                "the error names the backup: {words}"
            );
            let copy =
                Connection::open_with_flags(backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            assert_eq!(user_version(&copy), BEFORE_RESOLUTION);
            assert_eq!(item_cells(&copy, ITEM_COLUMNS_0007), before);
        }
        other => panic!("a migration failure, got {other:?}"),
    }
    let c = Connection::open(&path).unwrap();
    assert_eq!(user_version(&c), BEFORE_RESOLUTION, "the step rolled back");
    assert_eq!(item_cells(&c, ITEM_COLUMNS_0007), before);
    drop(c);

    assert!(
        matches!(
            open_project(dir.path()),
            Err(StoreError::MigrationFailed { from: 7, .. })
        ),
        "a later open retries the step"
    );
}

/// Row 37a (owner condition 5): a schema-7 database holding items of every
/// kind 3.3's writer accepts (AD-28's seven), on two runs (one running, one
/// failed), tainted and clean, with and without a payload, upgrades with no
/// item lost: the same number of items, the same ids and every 0007 column
/// equal value for value, each still `open`. The pre-migration copy the
/// step reports exists, opens at `user_version` 7 and holds the same items;
/// after the store is reopened it is still there, unchanged.
#[test]
fn migration_0008_loses_no_item_and_keeps_the_pre_migration_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_7(dir.path());
    {
        let c = Connection::open(&path).unwrap();
        for (r, (run, status)) in [("run-a", "running"), ("run-b", "failed")]
            .into_iter()
            .enumerate()
        {
            c.execute(
                "INSERT INTO runs (id, workflow, origin, status, created_at, updated_at) VALUES (?1, 'wf', 'manual', ?2, '2026-11-20T10:00:00.000Z', '2026-11-20T10:00:09.000Z')",
                params![run, status],
            )
            .unwrap();
            for (seq, key) in ["a", "b", "c"].into_iter().enumerate() {
                c.execute(
                    &format!("INSERT INTO step_runs ({STEP_COLUMNS_0007}) VALUES (?1, ?2, ?3, ?4, 1, 'succeeded', 'out', ?5, '2026-11-20T10:00:09.000Z', NULL, NULL)"),
                    params![format!("{run}-{key}"), run, key, seq as i64, (seq + r) as i64 % 2],
                )
                .unwrap();
            }
            for (i, kind) in InboxKind::ALL.into_iter().enumerate() {
                let step = format!("{run}-{}", ["a", "b", "c"][i % 3]);
                let payload = (i % 2 == 0).then(|| format!("{run}-a"));
                c.execute(
                    "INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at)
                     VALUES (?1, ?2, 'open', ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        format!("{run}-item-{}", kind.as_str()),
                        kind.as_str(),
                        run,
                        step,
                        payload,
                        ((i + r) % 2) as i64,
                        format!("{} item {i}", kind.title()),
                        format!("The engine's words for item {i} of {run}."),
                        format!("2026-11-20T10:00:0{i}.{r}00Z"),
                    ],
                )
                .unwrap();
            }
        }
    }
    let before = item_cells(&Connection::open(&path).unwrap(), ITEM_COLUMNS_0007);
    assert_eq!(before.len(), 14, "seven kinds on each of two runs");

    let (store, report) = open_project(dir.path()).unwrap();

    let step = report
        .applied
        .iter()
        .find(|m| m.from == BEFORE_RESOLUTION)
        .expect("the 7 to 8 step is reported");
    let backup = step.backup.clone().expect("a pre-migration copy");
    let c = Connection::open(&path).unwrap();
    assert_eq!(user_version(&c), BEFORE_RESOLUTION + 1);
    let after = item_cells(&c, ITEM_COLUMNS_0007);
    assert_eq!(after.len(), before.len(), "no item lost");
    assert_eq!(after, before, "the same ids, every 0007 column equal");
    drop(c);
    let reader = store.reader().unwrap();
    for run in ["run-a", "run-b"] {
        for item in reader.inbox_items(run).unwrap() {
            assert_eq!(item.status, "open", "{}", item.id);
            assert_eq!(item.decision, None, "{}", item.id);
            let want = (item.kind == InboxKind::Review).then_some(2);
            assert_eq!(item.max_returns, want, "{}", item.id);
        }
    }
    assert_eq!(reader.open_items().unwrap().len(), 14);

    let copy_holds_the_items = |what: &str| {
        assert!(backup.is_file(), "{what}: the copy exists");
        let copy = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(user_version(&copy), BEFORE_RESOLUTION, "{what}");
        assert_eq!(item_cells(&copy, ITEM_COLUMNS_0007), before, "{what}");
    };
    copy_holds_the_items("after the upgrade");
    drop(reader);
    drop(store);
    let (store, again) = open_project(dir.path()).unwrap();
    assert!(again.applied.is_empty());
    copy_holds_the_items("after a reopen");
    assert_eq!(
        item_cells(&Connection::open(&path).unwrap(), ITEM_COLUMNS_0007),
        before
    );
    drop(store);
}
