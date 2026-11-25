//! Trust test (AD-8, AD-10 as amended and restated 2026-11-24, AD-43;
//! story 3.22, review triage round 1, R3, R4 and R7): what a step reads at
//! its start is what its journal names, read once, at the start.
//!
//! - **R3:** a gate makes no model call and no person sees its rows, so a
//!   gate step that names rows in its `row_inputs` is refused in words
//!   before any run row, as a gate that declares a table is.
//! - **R4:** a step that names the same row twice does not start: the run
//!   fails in words naming the table and the row id, with a Failure item;
//!   no edge or part is journaled, twice or once; `run_plan` returns the
//!   outcome, never a bare store error.
//! - **R7:** the rows are read before `StartAttempt` commits, so a write
//!   elsewhere can move a row in between. The store refuses an edge that
//!   does not name the row's current after-image; the engine reads the rows
//!   again and tries once more. After one such move the step starts on the
//!   current image (its edge, its fenced part and the prompt the model gets
//!   all name it, and nothing of the first try is journaled); after a
//!   second move it fails in words naming the table and the row id.
//!
//! The move is forced through the plan's clock, with no engine seam of its
//! own: the engine reads `RunPlan.clock` once per try of an attempt's
//! start, after reading its rows and before `StartAttempt` (it dates the
//! prompt it assembles, AD-27). The test's clock updates the row from
//! another run's running step, through the store's row command, on each of
//! its first readings.
//!
//! FakeLlm only; no network, no real model. Written before the fixes.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use am_domain::rows::{RowOp, RowWrite};
use am_domain::{Actor, Clock, InboxKind, Taint};
use am_engine::run::{RunOutcome, StepPlan, run_plan};
use am_store::{AttemptWrites, Reader, RowRecord, WriteCommand, Writer, new_id};
use am_testkit::{FakeClock, FakeLlm, ScriptedTurn};
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

/// A value in the row's data no refusal, item or audit entry may hold.
const CANARY: &str = "canary-row-start-91d4";

/// The title each forced move writes, with the new version after it.
const MOVED: &str = "moved-by-another-run";

/// The seat and step of the other run that moves the row.
const MOVER_SEAT: &str = "seat-mover";
const MOVER_STEP: &str = "move";

async fn run(p: &Project, plan: &am_engine::run::RunPlan) -> RunOutcome {
    run_plan(&p.store, owner(), "manual", plan, CancellationToken::new())
        .await
        .expect("the run's outcome, never a bare store error")
}

/// Run 1: a clean step inserts one row holding [`CANARY`].
async fn written(p: &Project) -> RowRecord {
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("recording").tool_call(
            "table_write",
            &insert(json!({ "ref": "r-1", "title": CANARY })).to_string(),
        ),
        ScriptedTurn::text("recorded"),
    ]));
    let outcome = run(p, &plan(vec![writing(step("collect", llm))])).await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    only_row(&p.db, TABLE)
}

/// The attempt of step `key` of the run `run_id`.
fn attempt_of(p: &Project, run_id: &str, key: &str) -> am_store::StepRow {
    Reader::open(&p.db)
        .unwrap()
        .steps(run_id)
        .unwrap()
        .into_iter()
        .find(|s| s.step_key == key)
        .unwrap_or_else(|| panic!("no step {key} in run {run_id}"))
}

/// Nothing the run left names the row's data: its reason, its Inbox items
/// and the audit.
fn no_row_data(p: &Project, outcome: &RunOutcome) {
    let reason = outcome.reason.clone().unwrap_or_default();
    assert!(!reason.contains(CANARY), "{reason}");
    let j = journal(&p.db);
    for item in &j.items {
        assert!(!format!("{item:?}").contains(CANARY), "{item:?}");
    }
    for entry in &j.audit {
        assert!(!entry.to_string().contains(CANARY), "{entry}");
    }
}

/// The run failed before its step started: a Failure item, no model call,
/// and an attempt with no start time, no inputs and no prompt parts.
fn failed_before_start(p: &Project, outcome: &RunOutcome, model: &FakeLlm) {
    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let item = outcome.item.clone().expect("a Failure item");
    let j = journal(&p.db);
    let found = j
        .items
        .iter()
        .find(|i| i.id == item)
        .unwrap_or_else(|| panic!("the item {item}: {:#?}", j.items));
    assert_eq!(found.kind, InboxKind::Failure, "{found:?}");
    assert!(model.requests().is_empty(), "no model call");
    let attempt = attempt_of(p, &outcome.run_id, "read");
    assert_eq!(attempt.started_at, None, "{attempt:?}");
    assert!(j.inputs_of(&attempt.id).is_empty(), "no edge journaled");
    assert!(j.parts_of(&attempt.id).is_empty(), "no part journaled");
    no_row_data(p, outcome);
}

/// R3: a gate that names rows is refused in words naming the step, before
/// any run row, as a gate that declares a table is; no model is called and
/// the row is as it was.
#[tokio::test]
async fn a_gate_that_names_rows_is_refused_before_any_run() {
    let p = project();
    let row = written(&p).await;
    let runs = Reader::open(&p.db).unwrap().runs().unwrap().len();
    let draft = texts(&["drafted"]);

    let refused = run_plan(
        &p.store,
        owner(),
        "manual",
        &plan(vec![
            step("draft", draft.clone()),
            reading(gate("approve", never()), &[row.row_id.as_str()]),
        ]),
        CancellationToken::new(),
    )
    .await
    .expect_err("a gate reads no row");

    let words = refused.to_string();
    assert!(words.contains("approve"), "names the step: {words}");
    assert!(!words.contains(CANARY), "{words}");
    assert_eq!(
        Reader::open(&p.db).unwrap().runs().unwrap().len(),
        runs,
        "no run row"
    );
    assert!(draft.requests().is_empty(), "no model call");
    assert_eq!(only_row(&p.db, TABLE), row, "the row as it was");
}

/// R4: the same row named twice fails the run in words naming the table
/// and the row id, with a Failure item, before the step starts: nothing is
/// journaled for it, twice or once, and no row data leaves the row.
#[tokio::test]
async fn a_row_named_twice_fails_the_run_in_words() {
    let p = project();
    let row = written(&p).await;
    let model = texts(&["never"]);

    let outcome = run(
        &p,
        &plan(vec![reading(
            step("read", model.clone()),
            &[row.row_id.as_str(), row.row_id.as_str()],
        )]),
    )
    .await;

    failed_before_start(&p, &outcome, &model);
    let reason = outcome.reason.clone().unwrap_or_default();
    assert!(reason.contains(TABLE), "names the table: {reason}");
    assert!(reason.contains(&row.row_id), "names the row: {reason}");
    assert_eq!(only_row(&p.db, TABLE), row, "the row as it was");
}

/// Another run's step `move`, running and declared to write `entries` as
/// [`MOVER_SEAT`]: each move is one `table_write` call of its own, through
/// the store's row command, that updates a row to its next version.
struct Mover {
    db: PathBuf,
    writer: Writer,
    run: uuid::Uuid,
    attempt: String,
    calls: AtomicI64,
}

impl Mover {
    fn start(p: &Project) -> Arc<Mover> {
        let writer = p.store.writer();
        let run = uuid::Uuid::now_v7();
        writer
            .exec(
                engine(),
                WriteCommand::CreateRun {
                    run_id: run.to_string(),
                    workflow: "elsewhere".into(),
                    origin: "manual".into(),
                    steps: vec![MOVER_STEP.into()],
                    config_hash: None,
                },
            )
            .unwrap();
        let attempt = attempt_of(p, &run.to_string(), MOVER_STEP).id;
        writer
            .exec(
                engine(),
                WriteCommand::StartAttempt {
                    step_run_id: attempt.clone(),
                    inputs: vec![],
                    taint: Taint::Clean,
                    prompt: vec![],
                    writes: AttemptWrites {
                        seat: MOVER_SEAT.into(),
                        table: Some(TABLE.into()),
                        document: None,
                    },
                },
            )
            .unwrap();
        Arc::new(Mover {
            db: p.db.clone(),
            writer,
            run,
            attempt,
            calls: AtomicI64::new(0),
        })
    }

    /// Updates `row` from its current version to the next, its title
    /// `moved-by-another-run-<next version>`.
    fn move_row(&self, row: &str) {
        let version = Reader::open(&self.db)
            .unwrap()
            .table_row_by_id(TABLE, row)
            .unwrap()
            .expect("the row")
            .version;
        let call_seq = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let call = new_id();
        self.writer
            .exec(
                engine(),
                WriteCommand::RecordToolCall {
                    tool_call_id: call.clone(),
                    step_run_id: self.attempt.clone(),
                    call_seq,
                    tool: "table_write".into(),
                    status: "issued".into(),
                },
            )
            .unwrap();
        self.writer
            .apply_row_write(
                Actor::Seat {
                    seat: MOVER_SEAT.into(),
                    run: self.run,
                    step: MOVER_STEP.into(),
                },
                RowWrite {
                    tool_call_id: call,
                    step_run_id: self.attempt.clone(),
                    call_seq,
                    table_id: TABLE.into(),
                    op: RowOp::Update {
                        row_id: uuid::Uuid::parse_str(row).unwrap(),
                        expected_version: version,
                        patch_json: json!({ "title": format!("{MOVED}-{}", version + 1) })
                            .to_string(),
                    },
                    taint: Taint::Clean,
                },
            )
            .unwrap();
    }
}

/// The plan's clock: each of its first `moves` readings moves `row` (the
/// engine reads it once per try of an attempt's start, between reading the
/// rows and `StartAttempt`); every reading gives the fixture's time.
struct Racing {
    at: Arc<FakeClock>,
    mover: Arc<Mover>,
    row: String,
    moves: Mutex<usize>,
    readings: AtomicUsize,
}

impl Racing {
    fn new(mover: Arc<Mover>, row: &str, moves: usize) -> Arc<Racing> {
        Arc::new(Racing {
            at: FakeClock::at(CLOCK),
            mover,
            row: row.into(),
            moves: Mutex::new(moves),
            readings: AtomicUsize::new(0),
        })
    }
}

impl Clock for Racing {
    fn now(&self) -> jiff::Timestamp {
        self.readings.fetch_add(1, Ordering::SeqCst);
        let mut left = self.moves.lock().unwrap();
        if *left > 0 {
            *left -= 1;
            self.mover.move_row(&self.row);
        }
        self.at.now()
    }
}

/// A plan of one step `read` on `model` that reads `row`, dated by
/// `clock`.
fn read_plan(model: Arc<FakeLlm>, row: &str, clock: Arc<Racing>) -> am_engine::run::RunPlan {
    let steps: Vec<StepPlan> = vec![reading(step("read", model), &[row])];
    am_engine::run::RunPlan {
        clock,
        ..plan(steps)
    }
}

/// R7, one move: the row moves after the step read it and before its
/// start committed. The store refuses that start, the engine reads the row
/// again and starts once more, and succeeds: one attempt, started once,
/// whose one `row` edge, one fenced part and the one request its model
/// gets all name the row's current after-image (version 2), and nothing
/// names version 1. The clock was read twice at the start (two tries),
/// then once by the one call's reservation (story 3.7: its budget week).
#[tokio::test]
async fn a_row_that_moves_before_the_start_is_read_again_once() {
    let p = project();
    let row = written(&p).await;
    let mover = Mover::start(&p);
    let clock = Racing::new(mover, &row.row_id, 1);
    let model = texts(&["read it"]);

    let outcome = run(&p, &read_plan(model.clone(), &row.row_id, clock.clone())).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert_eq!(
        clock.readings.load(Ordering::SeqCst),
        3,
        "two tries, then the call's reservation (story 3.7)"
    );
    let now = only_row(&p.db, TABLE);
    assert_eq!(now.version, 2, "{now:?}");
    let image = history_of(&p.db, TABLE, &row.row_id)
        .last()
        .unwrap()
        .clone();
    let current = format!("row:{TABLE}/{}@2", row.row_id);
    let stale = format!("row:{TABLE}/{}@1", row.row_id);

    let j = journal(&p.db);
    let attempts = j.attempts("read");
    assert_eq!(attempts.len(), 1, "one attempt: {attempts:#?}");
    let attempt = &attempts[0];
    let edges: Vec<_> = j
        .inputs_of(&attempt.id)
        .into_iter()
        .filter(|i| i.kind == "row")
        .collect();
    assert_eq!(edges.len(), 1, "one edge: {edges:#?}");
    assert_eq!(edges[0].label, current);
    assert_eq!(edges[0].from_row, Some(image.seq), "the current image");
    let labels = j.labels_of(&attempt.id);
    assert_eq!(
        labels.iter().filter(|l| l.starts_with("row:")).count(),
        1,
        "one row part: {labels:?}"
    );
    assert!(
        labels.contains(&current) && !labels.contains(&stale),
        "{labels:?}"
    );
    let started: Vec<_> = j
        .entries("step.started")
        .into_iter()
        .filter(|e| e["target"] == attempt.id.as_str())
        .collect();
    assert_eq!(started.len(), 1, "started once: {:#?}", j.audit);

    let requests = model.requests();
    assert_eq!(requests.len(), 1, "one model call");
    let parts = user_parts(&requests[0]);
    let text = fenced(&parts, &current);
    assert_eq!(text, now.data, "the current image's data");
    assert!(text.contains(&format!("{MOVED}-2")), "{text}");
    assert!(
        format!("{:?}", requests[0]).matches(&stale).count() == 0,
        "nothing names version 1"
    );
}

/// R7, two moves: the row moves again between the second read and the
/// second start. The step fails in words naming the table and the row id,
/// before it starts (a Failure item, no model call, nothing journaled for
/// it); the engine tried twice, not three times; no row data leaves the
/// row.
#[tokio::test]
async fn a_row_that_moves_again_fails_the_step_in_words() {
    let p = project();
    let row = written(&p).await;
    let mover = Mover::start(&p);
    let clock = Racing::new(mover, &row.row_id, 2);
    let model = texts(&["never"]);

    let outcome = run(&p, &read_plan(model.clone(), &row.row_id, clock.clone())).await;

    failed_before_start(&p, &outcome, &model);
    assert_eq!(
        clock.readings.load(Ordering::SeqCst),
        2,
        "two tries, no third"
    );
    let reason = outcome.reason.clone().unwrap_or_default();
    assert!(reason.contains(TABLE), "names the table: {reason}");
    assert!(reason.contains(&row.row_id), "names the row: {reason}");
    assert!(!reason.contains(MOVED), "no row data: {reason}");
    assert_eq!(only_row(&p.db, TABLE).version, 3);
}
