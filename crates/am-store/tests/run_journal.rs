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

use am_domain::{Actor, RuntimeLayout, Usage};
use am_store::{OpenReport, Reader, SCHEMA_VERSION, Store, StoreError, WriteCommand, new_id};
use rusqlite::{Connection, params};
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
