//! Story 3.6c, "Rows reaching later steps", at the store (AD-7, AD-10 as
//! amended and restated 2026-11-24, AD-43; owner Q4's reader condition;
//! plan matrix rows 38, 45 and 47): a step reads a row through a `row`
//! input edge that names the after-image it read (`from_row`, its
//! `row_history.seq`) and carries at least that after-image's taint; the
//! row's text stays on the after-image.
//!
//! - `Reader::row_snapshot` is the row's current after-image (`seq`,
//!   version, data, taint): the one content reader the engine may name.
//! - A `row` edge names an existing after-image and no producing step run
//!   or Inbox item; its taint is raised to at least the after-image's (the
//!   mark is a floor); no other kind names an after-image.
//! - Its prompt part is fenced (`untrusted`, taint 1), never a trusted part,
//!   and carries no text: the text stays on the after-image (AD-43).
//! - Migration 0011 adds `step_inputs.from_row` behind AD-7's pre-migration
//!   copy, NULL on every old row; a failing 0011 leaves version 10.
//!
//! `a_row_input_edge_carries_the_rows_taint` moved here from 3.6's
//! `rows.rs` in the owner's re-slices (2026-11-24); the rest are 3.6c's own
//! tests-first step. The pinned API is in 3.6c's plan, Implementation
//! Notes. Written before the read side exists.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use am_domain::rows::{RowOp, RowWrite};
use am_domain::{Actor, RuntimeLayout, Taint};
use am_store::{
    AttemptWrites, CallPurpose, NewPromptPart, OpenReport, PromptKind, PromptSlot, Reader,
    RowHistoryRecord, RowRecord, RowSnapshot, SCHEMA_VERSION, StepInput, StepRow, Store,
    StoreError, WriteCommand, new_id,
};
use rusqlite::types::Value as Cell;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use uuid::Uuid;

const HASH: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";

/// The table every writing attempt declares, and another one.
const TABLE: &str = "entries";
const OTHER_TABLE: &str = "others";
/// The seat every attempt runs on.
const SEAT: &str = "seat-writer";

/// A value in a row's data that no refusal, prompt part or edge may hold.
const CANARY: &str = "canary-row-read-3c71";

/// The schema version before migration 0011.
const BEFORE_ROW_INPUTS: i64 = 10;

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

fn insert_op(row_id: Uuid, data: &Value) -> RowOp {
    RowOp::Insert {
        row_id,
        data_json: data.to_string(),
    }
}

fn writes(table: Option<&str>, document: Option<&str>) -> AttemptWrites {
    AttemptWrites {
        seat: SEAT.into(),
        table: table.map(str::to_string),
        document: document.map(str::to_string),
    }
}

/// A run's id, as text and as the UUID an `Actor::Seat` names.
#[derive(Debug, Clone)]
struct Run {
    id: String,
    uuid: Uuid,
}

/// One started step attempt.
#[derive(Debug, Clone)]
struct Attempt {
    run: Run,
    key: String,
    /// The step run's id.
    id: String,
}

impl Attempt {
    /// The seat acting in this attempt, as ToolGate names it.
    fn actor(&self) -> Actor {
        Actor::Seat {
            seat: SEAT.into(),
            run: self.run.uuid,
            step: self.key.clone(),
        }
    }
}

/// What a refused command must leave as it was.
#[derive(Debug, PartialEq)]
struct State {
    rows: Vec<RowRecord>,
    history: Vec<RowHistoryRecord>,
    steps: Vec<StepRow>,
    inputs: usize,
    audit: usize,
}

/// A store on a bare path, with one run of `steps`.
struct Rig {
    _dir: tempfile::TempDir,
    db: PathBuf,
    store: Store,
    run: Run,
}

fn rig_with(steps: &[&str]) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = Store::open(&db).unwrap();
    let mut r = Rig {
        _dir: dir,
        db,
        store,
        run: Run {
            id: String::new(),
            uuid: Uuid::nil(),
        },
    };
    r.run = r.new_run(steps);
    r
}

impl Rig {
    fn reader(&self) -> Reader {
        self.store.reader().unwrap()
    }

    fn exec(&self, cmd: WriteCommand) -> Result<(), StoreError> {
        self.store.writer().exec(engine(), cmd)
    }

    /// Another run of `steps` on the same store.
    fn new_run(&self, steps: &[&str]) -> Run {
        let uuid = Uuid::now_v7();
        let id = uuid.to_string();
        self.exec(WriteCommand::CreateRun {
            run_id: id.clone(),
            workflow: "wf".into(),
            origin: "manual".into(),
            steps: steps.iter().map(|s| s.to_string()).collect(),
            config_hash: Some(HASH.into()),
        })
        .unwrap();
        Run { id, uuid }
    }

    /// The latest attempt of step `key` of `run`.
    fn latest(&self, run: &Run, key: &str) -> StepRow {
        self.reader()
            .steps(&run.id)
            .unwrap()
            .into_iter()
            .filter(|s| s.step_key == key)
            .max_by_key(|s| s.attempt)
            .unwrap_or_else(|| panic!("no step {key}"))
    }

    /// One step attempt by id.
    fn step(&self, id: &str) -> StepRow {
        let reader = self.reader();
        reader
            .runs()
            .unwrap()
            .into_iter()
            .flat_map(|run| reader.steps(&run.id).unwrap())
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("no step run {id}"))
    }

    /// Starts the latest (queued) attempt of `key` with `inputs` and
    /// `prompt`, journaling its writes.
    fn start_with(
        &self,
        run: &Run,
        key: &str,
        writes: AttemptWrites,
        taint: Taint,
        inputs: Vec<StepInput>,
        prompt: Vec<NewPromptPart>,
    ) -> Result<Attempt, StoreError> {
        let row = self.latest(run, key);
        self.exec(WriteCommand::StartAttempt {
            step_run_id: row.id.clone(),
            inputs,
            taint,
            prompt,
            writes,
        })?;
        Ok(Attempt {
            run: run.clone(),
            key: key.into(),
            id: row.id,
        })
    }

    /// Starts the latest attempt of `key`, which may write `table`.
    fn begin(
        &self,
        run: &Run,
        key: &str,
        table: Option<&str>,
        document: Option<&str>,
        taint: Taint,
    ) -> Attempt {
        self.start_with(run, key, writes(table, document), taint, vec![], vec![])
            .unwrap()
    }

    /// Issues `table_write` call `seq` and inserts a new row holding
    /// `data`: its id.
    fn insert(&self, a: &Attempt, seq: i64, data: &Value) -> String {
        let call = new_id();
        self.exec(WriteCommand::RecordToolCall {
            tool_call_id: call.clone(),
            step_run_id: a.id.clone(),
            call_seq: seq,
            tool: "table_write".into(),
            status: "issued".into(),
        })
        .unwrap();
        let row = Uuid::now_v7();
        self.store
            .writer()
            .apply_row_write(
                a.actor(),
                RowWrite {
                    tool_call_id: call,
                    step_run_id: a.id.clone(),
                    call_seq: seq,
                    table_id: TABLE.into(),
                    op: insert_op(row, data),
                    taint: Taint::Clean,
                },
            )
            .unwrap();
        row.to_string()
    }

    /// Issues `table_write` call `seq` and updates `row`, read at `version`,
    /// with `patch`.
    fn update(&self, a: &Attempt, seq: i64, row: &str, version: i64, patch: &Value) {
        let call = new_id();
        self.exec(WriteCommand::RecordToolCall {
            tool_call_id: call.clone(),
            step_run_id: a.id.clone(),
            call_seq: seq,
            tool: "table_write".into(),
            status: "issued".into(),
        })
        .unwrap();
        self.store
            .writer()
            .apply_row_write(
                a.actor(),
                RowWrite {
                    tool_call_id: call,
                    step_run_id: a.id.clone(),
                    call_seq: seq,
                    table_id: TABLE.into(),
                    op: RowOp::Update {
                        row_id: Uuid::parse_str(row).unwrap(),
                        expected_version: version,
                        patch_json: patch.to_string(),
                    },
                    taint: Taint::Clean,
                },
            )
            .unwrap();
    }

    /// The attempt's call settled and the attempt `succeeded`.
    fn finish(&self, a: &Attempt) {
        let call = new_id();
        let seq = self.reader().calls_for_step(&a.id).unwrap().len() as i64 + 1;
        self.exec(WriteCommand::ReserveCall {
            step_run_id: a.id.clone(),
            call_id: call.clone(),
            call_seq: seq,
            price: am_domain::money::CallPrice::local(),
            rate: am_domain::money::Rate::none("EUR"),
            counted_input_tokens: 0,
            max_output_tokens: 0,
            week: "2026-11-23".into(),
            team: "core".into(),
            taint: Taint::Clean,
            purpose: CallPurpose::Turn,
            prompt: vec![],
        })
        .unwrap();
        self.exec(WriteCommand::SettleAndFinishStep {
            call_id: call,
            usage: None,
            step_run_id: a.id.clone(),
            status: "succeeded".into(),
            output: Some("done".into()),
        })
        .unwrap();
    }

    fn row(&self, row: &str) -> RowRecord {
        self.reader()
            .table_row_by_id(TABLE, row)
            .unwrap()
            .unwrap_or_else(|| panic!("no row {row}"))
    }

    fn history(&self, row: &str) -> Vec<RowHistoryRecord> {
        self.reader().row_history_of(TABLE, row).unwrap()
    }

    fn count(&self, table: &str) -> usize {
        let c = Connection::open_with_flags(&self.db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap() as usize
    }

    fn state(&self) -> State {
        let reader = self.reader();
        let rows = reader.table_rows_of(TABLE).unwrap();
        let history = rows
            .iter()
            .flat_map(|r| reader.row_history_of(&r.table_id, &r.row_id).unwrap())
            .collect();
        let mut steps = vec![];
        for run in reader.runs().unwrap() {
            steps.extend(reader.steps(&run.id).unwrap());
        }
        State {
            rows,
            history,
            steps,
            inputs: self.count("step_inputs"),
            audit: self.count("audit_log"),
        }
    }
}

/// Row 38 at the store (AD-10, AD-43): a `row` input names an after-image
/// (`from_row`, its `seq`) and carries at least its taint: a tainted row's
/// edge is tainted (and so is the reading attempt) even when the command
/// says clean; a clean row's edge stays clean, and its fenced prompt part
/// taints the attempt (3.5's rule). An edge that names no after-image, or
/// that also names a producing step run, is refused with nothing written.
#[test]
fn a_row_input_edge_carries_the_rows_taint() {
    let r = rig_with(&["collect", "check"]);
    let tainted = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Tainted);
    let dirty = r.insert(&tainted, 1, &json!({ "ref": "r-1" }));
    r.finish(&tainted);
    let clean = r.begin(&r.run, "check", Some(TABLE), None, Taint::Clean);
    let fine = r.insert(&clean, 1, &json!({ "ref": "r-2" }));
    r.finish(&clean);
    let seq_of = |row: &str| r.history(row).last().unwrap().seq;
    let label = |row: &str| format!("row:{TABLE}/{row}@1");
    let row_input = |row: &str, from_row: i64| StepInput {
        kind: "row".into(),
        from_step_run: None,
        from_item: None,
        from_row: Some(from_row),
        label: label(row),
        taint: Taint::Clean,
    };
    let later = r.new_run(&["first", "second", "third"]);

    let x = r
        .start_with(
            &later,
            "first",
            writes(None, None),
            Taint::Clean,
            vec![row_input(&dirty, seq_of(&dirty))],
            vec![],
        )
        .unwrap();
    let edges = r.reader().step_inputs(&x.id).unwrap();
    assert_eq!(
        edges,
        vec![StepInput {
            taint: Taint::Tainted,
            ..row_input(&dirty, seq_of(&dirty))
        }],
        "the edge carries the after-image's taint"
    );
    assert_eq!(r.step(&x.id).taint, Taint::Tainted, "the reader is tainted");
    assert_eq!(r.row(&dirty).taint, Taint::Tainted, "the mark is unchanged");
    r.finish(&x);

    let fenced = vec![
        NewPromptPart {
            slot: PromptSlot::Step,
            kind: PromptKind::Instruction,
            label: "Step instruction".into(),
            taint: Taint::Clean,
            text: Some("Read the row.".into()),
        },
        NewPromptPart {
            slot: PromptSlot::Untrusted,
            kind: PromptKind::Untrusted,
            label: label(&fine),
            taint: Taint::Tainted,
            text: None,
        },
    ];
    let y = r
        .start_with(
            &later,
            "second",
            writes(None, None),
            Taint::Clean,
            vec![row_input(&fine, seq_of(&fine))],
            fenced,
        )
        .unwrap();
    let edges = r.reader().step_inputs(&y.id).unwrap();
    assert_eq!(edges, vec![row_input(&fine, seq_of(&fine))], "a clean edge");
    assert_eq!(
        r.step(&y.id).taint,
        Taint::Tainted,
        "its fenced text taints the reader"
    );
    r.finish(&y);

    let third = r.latest(&later, "third");
    for (what, input) in [
        ("naming no after-image", row_input(&fine, 999_999)),
        (
            "naming a producing step run",
            StepInput {
                from_step_run: Some(clean.id.clone()),
                ..row_input(&fine, seq_of(&fine))
            },
        ),
        (
            "naming no row at all",
            StepInput {
                from_row: None,
                ..row_input(&fine, seq_of(&fine))
            },
        ),
    ] {
        let before = r.state();
        assert!(
            r.start_with(
                &later,
                "third",
                writes(None, None),
                Taint::Clean,
                vec![input],
                vec![]
            )
            .is_err(),
            "{what}: refused"
        );
        assert_eq!(r.state(), before, "{what}: nothing written");
        assert_eq!(r.step(&third.id).status, "queued", "{what}");
    }
}

/// A `row` edge for `row` of [`TABLE`] at version 1, naming `from_row`,
/// with the taint the command says.
fn row_edge(row: &str, from_row: i64, taint: Taint) -> StepInput {
    StepInput {
        kind: "row".into(),
        from_step_run: None,
        from_item: None,
        from_row: Some(from_row),
        label: format!("row:{TABLE}/{row}@1"),
        taint,
    }
}

/// The one Step part every assembled prompt holds (AD-27).
fn step_part() -> NewPromptPart {
    NewPromptPart {
        slot: PromptSlot::Step,
        kind: PromptKind::Instruction,
        label: "Step instruction".into(),
        taint: Taint::Clean,
        text: Some("Read the row.".into()),
    }
}

/// A row's prompt part, labelled `label`, as the engine journals it:
/// fenced, tainted, its text left on the after-image (`text: None`).
fn row_part(label: &str, text: Option<String>) -> NewPromptPart {
    NewPromptPart {
        slot: PromptSlot::Untrusted,
        kind: PromptKind::Untrusted,
        label: label.into(),
        taint: Taint::Tainted,
        text,
    }
}

/// Row 38 (Design Notes, "Reading rows"): `Reader::row_snapshot` is a
/// row's current after-image: its `seq`, version, data and taint, as the
/// newest `row_history` image and the row hold them, after an update too;
/// `None` for a row id the table does not hold, and for a row of another
/// table asked under this one. Reading changes nothing.
#[test]
fn row_snapshot_is_the_rows_current_after_image() {
    let r = rig_with(&["collect", "revise"]);
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let id = r.insert(&a, 1, &json!({ "ref": "r-1", "title": CANARY }));
    r.finish(&a);
    let first = r
        .reader()
        .row_snapshot(TABLE, &id)
        .unwrap()
        .expect("the row");
    let images = r.history(&id);
    assert_eq!(
        first,
        RowSnapshot {
            seq: images[0].seq,
            version: 1,
            data: images[0].data.clone(),
            taint: Taint::Clean,
        }
    );

    let b = r.begin(&r.run, "revise", Some(TABLE), None, Taint::Tainted);
    r.update(&b, 1, &id, 1, &json!({ "state": "done" }));
    r.finish(&b);
    let before = r.state();

    let now = r
        .reader()
        .row_snapshot(TABLE, &id)
        .unwrap()
        .expect("the row");

    let images = r.history(&id);
    assert_eq!(images.len(), 2);
    let newest = images.last().unwrap();
    assert_eq!(
        now,
        RowSnapshot {
            seq: newest.seq,
            version: 2,
            data: newest.data.clone(),
            taint: Taint::Tainted,
        },
        "the current after-image"
    );
    assert!(now.seq > first.seq);
    assert_eq!(now.data, r.row(&id).data, "the row's data as stored");
    assert_eq!(
        r.reader()
            .row_snapshot(TABLE, &Uuid::now_v7().to_string())
            .unwrap(),
        None,
        "a row the table does not hold"
    );
    assert_eq!(
        r.reader().row_snapshot(OTHER_TABLE, &id).unwrap(),
        None,
        "a row of another table"
    );
    assert_eq!(r.state(), before, "reading changes nothing");
}

/// Row 47 (AD-10 as restated: the mark is a floor; AD-43): the writer
/// never journals a `row` edge below its after-image's taint: a tainted
/// image's edge sent clean is journaled tainted (and taints the reader),
/// and a clean image's edge sent tainted stays tainted (a floor, never a
/// ceiling). Refused, with nothing written and the attempt still queued,
/// and in words that hold no row data: a `row` edge that also names an
/// Inbox item, or an after-image that does not exist; a step output or a
/// plan input that names an after-image (only a `row` edge does).
#[test]
fn a_row_edge_needs_an_existing_after_image_and_no_lower_taint() {
    let r = rig_with(&["collect", "check"]);
    let tainted = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Tainted);
    let dirty = r.insert(&tainted, 1, &json!({ "ref": "r-1", "title": CANARY }));
    r.finish(&tainted);
    let clean = r.begin(&r.run, "check", Some(TABLE), None, Taint::Clean);
    let fine = r.insert(&clean, 1, &json!({ "ref": "r-2", "title": CANARY }));
    r.finish(&clean);
    let seq_of = |row: &str| r.history(row).last().unwrap().seq;
    let later = r.new_run(&["low", "high", "bad"]);

    let low = r
        .start_with(
            &later,
            "low",
            writes(None, None),
            Taint::Clean,
            vec![row_edge(&dirty, seq_of(&dirty), Taint::Clean)],
            vec![],
        )
        .unwrap();
    assert_eq!(
        r.reader().step_inputs(&low.id).unwrap(),
        vec![row_edge(&dirty, seq_of(&dirty), Taint::Tainted)],
        "a tainted image's edge is never journaled clean"
    );
    assert_eq!(r.step(&low.id).taint, Taint::Tainted);
    r.finish(&low);

    let high = r
        .start_with(
            &later,
            "high",
            writes(None, None),
            Taint::Clean,
            vec![row_edge(&fine, seq_of(&fine), Taint::Tainted)],
            vec![],
        )
        .unwrap();
    assert_eq!(
        r.reader().step_inputs(&high.id).unwrap(),
        vec![row_edge(&fine, seq_of(&fine), Taint::Tainted)],
        "a floor, never a ceiling"
    );
    r.finish(&high);
    assert_eq!(
        r.row(&dirty).taint,
        Taint::Tainted,
        "no read changes a mark"
    );
    assert_eq!(r.row(&fine).taint, Taint::Clean, "no read changes a mark");

    let bad = r.latest(&later, "bad");
    let newest = seq_of(&dirty).max(seq_of(&fine));
    for (what, input) in [
        (
            "a row edge that also names an Inbox item",
            StepInput {
                from_item: Some(new_id()),
                ..row_edge(&fine, seq_of(&fine), Taint::Clean)
            },
        ),
        (
            "a row edge naming an after-image that does not exist",
            row_edge(&fine, newest + 1_000, Taint::Clean),
        ),
        (
            "a step output naming an after-image",
            StepInput {
                kind: "step_output".into(),
                from_step_run: Some(low.id.clone()),
                from_item: None,
                from_row: Some(seq_of(&fine)),
                label: "step:low@1".into(),
                taint: Taint::Tainted,
            },
        ),
        (
            "a plan input naming an after-image",
            StepInput {
                kind: "untrusted".into(),
                from_step_run: None,
                from_item: None,
                from_row: Some(seq_of(&fine)),
                label: "test:page".into(),
                taint: Taint::Tainted,
            },
        ),
    ] {
        let before = r.state();
        let refused = r
            .start_with(
                &later,
                "bad",
                writes(None, None),
                Taint::Clean,
                vec![input],
                vec![],
            )
            .expect_err(what);
        let words = refused.to_string();
        assert!(!words.contains(CANARY), "{what}: no row data: {words}");
        assert_eq!(r.state(), before, "{what}: nothing written");
        assert_eq!(r.step(&bad.id).status, "queued", "{what}");
    }
}

/// Row 47 (AD-43: the text is never stored twice): a row's fenced prompt
/// part is journaled `untrusted` in the untrusted slot, taint 1, with no
/// text and 0 bytes (its text stays on the after-image); no journaled part
/// or edge label holds the row's data. A row part that carries the row's
/// text is refused in words that hold none of it, and nothing is written.
#[test]
fn the_row_parts_text_is_not_journaled() {
    let r = rig_with(&["collect"]);
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let id = r.insert(&a, 1, &json!({ "ref": "r-1", "title": CANARY }));
    r.finish(&a);
    let image = r.history(&id).last().unwrap().clone();
    let label = format!("row:{TABLE}/{id}@1");
    let later = r.new_run(&["read", "again"]);

    let x = r
        .start_with(
            &later,
            "read",
            writes(None, None),
            Taint::Clean,
            vec![row_edge(&id, image.seq, Taint::Clean)],
            vec![step_part(), row_part(&label, None)],
        )
        .unwrap();

    let parts = r.reader().prompt_parts(&x.id).unwrap();
    let part = parts
        .iter()
        .find(|p| p.label == label)
        .unwrap_or_else(|| panic!("the row's part: {parts:#?}"));
    assert_eq!(
        (
            part.slot,
            part.kind,
            part.taint,
            part.text.clone(),
            part.bytes
        ),
        (
            PromptSlot::Untrusted,
            PromptKind::Untrusted,
            Taint::Tainted,
            None,
            0
        )
    );
    for p in &parts {
        assert!(
            !p.text.as_deref().unwrap_or_default().contains(CANARY),
            "{p:?}"
        );
    }
    for edge in r.reader().step_inputs(&x.id).unwrap() {
        assert!(!edge.label.contains(CANARY), "{edge:?}");
    }
    assert_eq!(r.step(&x.id).taint, Taint::Tainted, "the fence taints it");
    r.finish(&x);

    let again = r.latest(&later, "again");
    let before = r.state();
    let refused = r
        .start_with(
            &later,
            "again",
            writes(None, None),
            Taint::Clean,
            vec![row_edge(&id, image.seq, Taint::Clean)],
            vec![step_part(), row_part(&label, Some(image.data.clone()))],
        )
        .expect_err("a row part carries no text");
    let words = refused.to_string();
    assert!(!words.contains(CANARY), "{words}");
    assert_eq!(r.state(), before, "nothing written");
    assert_eq!(r.step(&again.id).status, "queued");
}

/// Row 47 (AD-10: a row read never becomes a `TrustedInput`; added by the
/// tester from the plan's Boundaries): a clean row's edge (taint 0) shown
/// as a trusted part is refused, though a trusted part may otherwise name
/// a clean input; and a fenced part labelled as a row no `row` edge of the
/// attempt names is refused. Nothing is written either time. The control:
/// the same edge shown as a fenced part starts the attempt, tainted.
#[test]
fn a_row_edge_is_never_a_trusted_part() {
    let r = rig_with(&["collect"]);
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let id = r.insert(&a, 1, &json!({ "ref": "r-1", "title": CANARY }));
    r.finish(&a);
    let seq = r.history(&id).last().unwrap().seq;
    let label = format!("row:{TABLE}/{id}@1");
    let later = r.new_run(&["read"]);
    let read = r.latest(&later, "read");

    for (what, inputs, shown) in [
        (
            "a clean row's edge as a trusted part",
            vec![row_edge(&id, seq, Taint::Clean)],
            NewPromptPart {
                slot: PromptSlot::Trusted,
                kind: PromptKind::Trusted,
                label: label.clone(),
                taint: Taint::Clean,
                text: None,
            },
        ),
        (
            "a row part with no row edge",
            vec![],
            row_part(&label, None),
        ),
    ] {
        let before = r.state();
        let refused = r
            .start_with(
                &later,
                "read",
                writes(None, None),
                Taint::Clean,
                inputs,
                vec![step_part(), shown],
            )
            .expect_err(what);
        assert!(!refused.to_string().contains(CANARY), "{what}: {refused}");
        assert_eq!(r.state(), before, "{what}: nothing written");
        assert_eq!(r.step(&read.id).status, "queued", "{what}");
    }

    // The control: the same edge, shown fenced, starts the attempt.
    let started = r
        .start_with(
            &later,
            "read",
            writes(None, None),
            Taint::Clean,
            vec![row_edge(&id, seq, Taint::Clean)],
            vec![step_part(), row_part(&label, None)],
        )
        .unwrap();
    assert_eq!(r.step(&started.id).taint, Taint::Tainted);
}

// ------------------------------------------------------------ migration 0011

fn layout(root: &Path) -> RuntimeLayout {
    RuntimeLayout::at(root.join("rt"))
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

fn user_version(conn: &Connection) -> i64 {
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
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

/// A schema-10 database in the project's runtime folder, made by hand.
fn database_at_10(root: &Path) -> PathBuf {
    existing_runtime(root);
    let path = layout(root).state_db();
    am_store::testing::database_at_version(&path, BEFORE_ROW_INPUTS).unwrap();
    path
}

/// The id of the row the fixture's seat wrote, and of a person's row.
const ROW_1: &str = "0193f1a2-0000-7000-8000-00000000a001";
const ROW_2: &str = "0193f1a2-0000-7000-8000-00000000a002";

/// Rows in every table a run touches, as 3.3's to the ToolGate slice's
/// writers made them at version 10: two runs (one running, one failed);
/// attempts with their journaled writes (seat, table); settled and open
/// calls (a turn and a repair) with their ledger rows; inputs (a step
/// output, a plan input, a gate note); tool calls with each result kind
/// (`engine` for two row writes, `untrusted` for a read, none for a
/// refused call); a seat's row with two after-images and a person's row
/// with one; an open Review and a dismissed Failure item; prompt parts; a
/// trigger and a fire.
fn rows_at_10(path: &Path) {
    let step = "id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at";
    let call = "id, step_run_id, call_seq, status, settled, input_tokens, output_tokens, created_at, purpose";
    let tool = "id, step_run_id, call_seq, tool, status, created_at, finished_at";
    let image = "table_id, row_id, version, data, taint, actor, run_id, step_run_id, seat, call_seq, tool_call_id, op, created_at";
    let row = "table_id, row_id, data, version, taint, created_at, updated_at";
    let part =
        "step_run_id, call_seq, ord, slot, kind, label, taint, text, bytes, sha256, created_at";
    let sql = r#"
        INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
          VALUES ('r1', 'wf', 'manual', 'running', '2026-11-24T10:00:00.000Z', '2026-11-24T10:00:09.000Z', '@HASH@', NULL);
        INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
          VALUES ('r2', 'wf', 'schedule', 'failed', '2026-11-24T11:00:00.000Z', '2026-11-24T11:00:09.000Z', NULL, '2026-11-24T11:00:09.000Z');
        INSERT INTO step_runs (@STEP@)
          VALUES ('r1-a', 'r1', 'a', 0, 1, 'queued', NULL, 0, '2026-11-24T10:00:00.000Z', NULL, NULL);
        INSERT INTO step_runs (@STEP@)
          VALUES ('r1-b', 'r1', 'b', 1, 1, 'queued', NULL, 0, '2026-11-24T10:00:00.000Z', NULL, NULL);
        INSERT INTO step_runs (@STEP@)
          VALUES ('r1-c', 'r1', 'c', 2, 1, 'queued', NULL, 0, '2026-11-24T10:00:00.000Z', NULL, NULL);
        INSERT INTO step_runs (@STEP@)
          VALUES ('r2-a', 'r2', 'a', 0, 1, 'queued', NULL, 0, '2026-11-24T11:00:00.000Z', NULL, NULL);
        UPDATE step_runs SET status = 'running', seat = 'seat-writer', writes_table = 'entries', taint = 1,
          started_at = '2026-11-24T10:00:01.000Z', updated_at = '2026-11-24T10:00:01.000Z' WHERE id = 'r1-a';
        INSERT INTO tool_calls (@TOOL@)
          VALUES ('t1', 'r1-a', 1, 'table_write', 'issued', '2026-11-24T10:00:02.000Z', NULL);
        INSERT INTO row_history (@IMAGE@)
          VALUES ('entries', '@ROW1@', 1, '{"ref":"r-1"}', 1, '@SEAT@', 'r1', 'r1-a', 'seat-writer', 1, 't1', 'insert', '2026-11-24T10:00:02.100Z');
        INSERT INTO table_rows (@ROW@)
          VALUES ('entries', '@ROW1@', '{"ref":"r-1"}', 1, 1, '2026-11-24T10:00:02.100Z', '2026-11-24T10:00:02.100Z');
        UPDATE tool_calls SET status = 'succeeded', finished_at = '2026-11-24T10:00:02.100Z',
          result_kind = 'engine', result_label = 'tool:table_write' WHERE id = 't1';
        INSERT INTO tool_calls (@TOOL@)
          VALUES ('t2', 'r1-a', 2, 'table_write', 'issued', '2026-11-24T10:00:02.200Z', NULL);
        INSERT INTO row_history (@IMAGE@)
          VALUES ('entries', '@ROW1@', 2, '{"ref":"r-1","state":"done"}', 1, '@SEAT@', 'r1', 'r1-a', 'seat-writer', 2, 't2', 'update', '2026-11-24T10:00:02.300Z');
        UPDATE table_rows SET data = '{"ref":"r-1","state":"done"}', version = 2, updated_at = '2026-11-24T10:00:02.300Z'
          WHERE table_id = 'entries' AND row_id = '@ROW1@';
        UPDATE tool_calls SET status = 'succeeded', finished_at = '2026-11-24T10:00:02.300Z',
          result_kind = 'engine', result_label = 'tool:table_write' WHERE id = 't2';
        UPDATE step_runs SET status = 'succeeded', output = 'out of a', finished_at = '2026-11-24T10:00:03.000Z',
          updated_at = '2026-11-24T10:00:03.000Z' WHERE id = 'r1-a';
        INSERT INTO row_history (@IMAGE@)
          VALUES ('others', '@ROW2@', 1, '{"ref":"o-1"}', 0, '@HUMAN@', NULL, NULL, NULL, NULL, NULL, 'insert', '2026-11-24T10:00:03.500Z');
        INSERT INTO table_rows (@ROW@)
          VALUES ('others', '@ROW2@', '{"ref":"o-1"}', 1, 0, '2026-11-24T10:00:03.500Z', '2026-11-24T10:00:03.500Z');
        UPDATE step_runs SET status = 'running', seat = 'seat-writer', taint = 1,
          started_at = '2026-11-24T10:00:04.000Z', updated_at = '2026-11-24T10:00:09.000Z' WHERE id = 'r1-b';
        INSERT INTO tool_calls (@TOOL@)
          VALUES ('t3', 'r1-b', 1, 'fixture_read', 'issued', '2026-11-24T10:00:06.000Z', NULL);
        UPDATE tool_calls SET status = 'succeeded', finished_at = '2026-11-24T10:00:07.000Z',
          result_kind = 'untrusted', result_label = 'tool:fixture_read' WHERE id = 't3';
        INSERT INTO tool_calls (@TOOL@)
          VALUES ('t4', 'r1-b', 2, 'fixture_act', 'refused', '2026-11-24T10:00:08.000Z', '2026-11-24T10:00:08.000Z');
        UPDATE step_runs SET status = 'failed', started_at = '2026-11-24T11:00:01.000Z',
          finished_at = '2026-11-24T11:00:09.000Z', updated_at = '2026-11-24T11:00:09.000Z' WHERE id = 'r2-a';
        INSERT INTO model_calls (@CALL@)
          VALUES ('c1', 'r1-a', 1, 'settled', 1, 12, 7, '2026-11-24T10:00:02.000Z', 'turn');
        INSERT INTO model_calls (@CALL@)
          VALUES ('c4', 'r1-a', 2, 'settled', 1, 9, 4, '2026-11-24T10:00:02.500Z', 'repair');
        INSERT INTO model_calls (@CALL@)
          VALUES ('c2', 'r1-b', 1, 'reserved', 0, NULL, NULL, '2026-11-24T10:00:05.000Z', 'turn');
        INSERT INTO model_calls (@CALL@)
          VALUES ('c3', 'r2-a', 1, 'settled', 1, 3, 0, '2026-11-24T11:00:02.000Z', 'turn');
        INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at)
          VALUES ('l1', 'c1', 'r1', 'model_call', 0, 0, 'settled', 'none', '2026-11-24T10:00:02.000Z');
        INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at)
          VALUES ('l2', 'c2', 'r1', 'model_call', 0, NULL, 'reserved', 'none', '2026-11-24T10:00:05.000Z');
        INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at)
          VALUES ('l3', 'c3', 'r2', 'model_call', 0, 0, 'settled', 'none', '2026-11-24T11:00:02.000Z');
        INSERT INTO step_inputs (step_run_id, kind, from_step_run, label, taint, created_at, from_item)
          VALUES ('r1-b', 'step_output', 'r1-a', 'step:a@1', 1, '2026-11-24T10:00:04.000Z', NULL);
        INSERT INTO step_inputs (step_run_id, kind, from_step_run, label, taint, created_at, from_item)
          VALUES ('r1-b', 'untrusted', NULL, 'test:page', 1, '2026-11-24T10:00:04.000Z', NULL);
        INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, max_returns)
          VALUES ('i1', 'review', 'open', 'r1', 'r1-b', 'r1-a', 1, 'Review', 'Step b waits for your review.', '2026-11-24T10:00:09.000Z', 2);
        INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, resolved_at, resolved_by, decision)
          VALUES ('i2', 'failure', 'resolved', 'r2', 'r2-a', 'r2-a', 0, 'A step stopped', 'Step a stopped.', '2026-11-24T11:00:09.000Z', '2026-11-24T11:05:00.000Z', '@HUMAN@', 'dismiss');
        INSERT INTO step_inputs (step_run_id, kind, from_step_run, label, taint, created_at, from_item)
          VALUES ('r1-c', 'gate_note', NULL, 'gate-note:b@1', 0, '2026-11-24T10:00:10.000Z', 'i1');
        INSERT INTO prompt_parts (@PART@)
          VALUES ('r1-b', 0, 1, 'step', 'instruction', 'Step instruction', 0, 'Do the work of b.', 17, NULL, '2026-11-24T10:00:04.000Z');
        INSERT INTO prompt_parts (@PART@)
          VALUES ('r1-b', 0, 2, 'untrusted', 'untrusted', 'step:a@1', 1, NULL, 0, NULL, '2026-11-24T10:00:04.000Z');
        INSERT INTO prompt_parts (@PART@)
          VALUES ('r1-b', 0, 3, 'untrusted', 'untrusted', 'test:page', 1, 'a page', 6, NULL, '2026-11-24T10:00:04.000Z');
        INSERT INTO triggers (id, cron, tz, catch_up, state, next_fire, last_fired)
          VALUES ('trg', '0 9 * * *', 'UTC', 'once', 'active', '2026-11-25T09:00:00.000Z', NULL);
        INSERT INTO fires (trigger_id, scheduled_for, fired_at, kind)
          VALUES ('trg', '2026-11-24T09:00:00.000Z', '2026-11-24T09:00:01.000Z', 'on_time');
    "#;
    let seat = json!({
        "kind": "seat",
        "seat": SEAT,
        "run": "0193f1a2-0000-7000-8000-000000000001",
        "step": "a",
    })
    .to_string();
    let human = json!({ "kind": "human", "person": "owner", "hat": "Reviewer" }).to_string();
    let sql = sql
        .replace("@STEP@", step)
        .replace("@CALL@", call)
        .replace("@TOOL@", tool)
        .replace("@IMAGE@", image)
        .replace("@ROW@", row)
        .replace("@PART@", part)
        .replace("@HASH@", HASH)
        .replace("@ROW1@", ROW_1)
        .replace("@ROW2@", ROW_2)
        .replace("@SEAT@", &seat)
        .replace("@HUMAN@", &human);
    Connection::open(path).unwrap().execute_batch(&sql).unwrap();
}

/// Every table's columns and rows (raw cells, in rowid order) as `path`
/// holds them now.
type Tables = BTreeMap<String, (Vec<String>, Vec<Vec<Cell>>)>;

fn tables_of(path: &Path) -> Tables {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut out = BTreeMap::new();
    for name in names {
        let cols = columns(path, &name);
        out.insert(name.clone(), (cols.clone(), cells_of(&conn, &name, &cols)));
    }
    out
}

fn cells_of(conn: &Connection, table: &str, cols: &[String]) -> Vec<Vec<Cell>> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {} FROM \"{table}\" ORDER BY rowid",
            cols.join(", ")
        ))
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

/// The number of files under `dir`, recursively (0 when it is missing).
fn walk_count(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() { walk_count(&p) } else { 1 }
        })
        .sum()
}

/// Row 45: every row of every table the schema-10 database held is the
/// same value for value after 0011 (in its old columns; the two logs a
/// migration appends to keep their old rows as a prefix); the
/// pre-migration copy the report names exists, opens at version 10 and
/// holds every row as it was, also after a reopen; the step is reported
/// 10 to 11 and audited `store.migrated`.
#[test]
fn migration_0011_keeps_every_row_and_the_pre_migration_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_10(dir.path());
    rows_at_10(&path);
    let before = tables_of(&path);
    for table in [
        "runs",
        "step_runs",
        "model_calls",
        "ledger",
        "step_inputs",
        "tool_calls",
        "inbox_items",
        "prompt_parts",
        "table_rows",
        "row_history",
        "triggers",
        "fires",
    ] {
        assert!(!before[table].1.is_empty(), "{table} holds rows to keep");
    }
    assert!(
        !before["step_inputs"].0.contains(&"from_row".to_string()),
        "from_row is 0011's"
    );

    let (store, report) = open_project(dir.path()).unwrap();

    // 3.6b adds 0012 after it: the 10 to 11 step is applied on the way,
    // not necessarily last.
    const { assert!(SCHEMA_VERSION > BEFORE_ROW_INPUTS) };
    assert_eq!(report.version, SCHEMA_VERSION);
    let step = report
        .applied
        .iter()
        .find(|m| m.from == BEFORE_ROW_INPUTS)
        .expect("the 10 to 11 step is reported");
    assert_eq!(step.to, BEFORE_ROW_INPUTS + 1);
    let backup = step.backup.clone().expect("a pre-migration copy (AD-7)");
    assert!(backup.is_file(), "{}", backup.display());
    assert!(
        report
            .lines()
            .iter()
            .any(|l| l.contains("from version 10 to 11")),
        "{:?}",
        report.lines()
    );

    let after = tables_of(&path);
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    for (table, (cols, rows)) in &before {
        let (_, now) = after
            .get(table)
            .unwrap_or_else(|| panic!("{table} was dropped"));
        let old_columns_now = cells_of(&conn, table, cols);
        if table == "audit_log" || table == "migration_reports" {
            assert!(
                old_columns_now.starts_with(rows),
                "{table}: the old entries are kept, in order"
            );
        } else {
            assert_eq!(
                &old_columns_now, rows,
                "{table}: every row, value for value"
            );
            assert_eq!(now.len(), rows.len(), "{table}: no row added or lost");
        }
    }
    drop(conn);

    let copy_is_as_it_was = |what: &str| {
        let copy = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(user_version(&copy), BEFORE_ROW_INPUTS, "{what}");
        drop(copy);
        assert_eq!(
            tables_of(&backup),
            before,
            "{what}: the copy holds every row"
        );
    };
    copy_is_as_it_was("after the upgrade");

    let reader = store.reader().unwrap();
    let mut out = Vec::new();
    reader.export_audit_jsonl(&mut out).unwrap();
    let all: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        all.iter().any(|e| e["action"] == "store.migrated"
            && e["outcome"]
                .as_str()
                .is_some_and(|o| o.starts_with("from version 10 to 11"))),
        "{all:#?}"
    );
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
    drop(reader);
    drop(store);
    let (_store, again) = open_project(dir.path()).unwrap();
    assert!(again.applied.is_empty(), "reopening applies nothing");
    copy_is_as_it_was("after a reopen");
}

/// Row 45: 0011 adds `step_inputs.from_row`, a reference to an after-image
/// (`row_history(seq)`), NULL on every old input; the reader reads every
/// old input as before with `from_row: None`, and the rows, their history
/// and their snapshots as they were. A database made at exactly version 11
/// has no `document_versions` (3.6b's 0012).
#[test]
fn migration_0011_adds_a_null_from_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_10(dir.path());
    rows_at_10(&path);

    let (store, _) = open_project(dir.path()).unwrap();

    assert!(
        columns(&path, "step_inputs").contains(&"from_row".to_string()),
        "{:?}",
        columns(&path, "step_inputs")
    );
    let c = Connection::open(&path).unwrap();
    let references: Vec<(String, String, String)> = c
        .prepare("SELECT \"from\", \"table\", \"to\" FROM pragma_foreign_key_list('step_inputs')")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        references.contains(&("from_row".into(), "row_history".into(), "seq".into())),
        "from_row names an after-image: {references:?}"
    );
    let set: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM step_inputs WHERE from_row IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(set, 0, "no old input names an after-image");
    drop(c);

    let reader = store.reader().unwrap();
    assert_eq!(
        reader.step_inputs("r1-b").unwrap(),
        vec![
            StepInput {
                kind: "step_output".into(),
                from_step_run: Some("r1-a".into()),
                from_item: None,
                from_row: None,
                label: "step:a@1".into(),
                taint: Taint::Tainted,
            },
            StepInput {
                kind: "untrusted".into(),
                from_step_run: None,
                from_item: None,
                from_row: None,
                label: "test:page".into(),
                taint: Taint::Tainted,
            },
        ]
    );
    assert_eq!(
        reader.step_inputs("r1-c").unwrap(),
        vec![StepInput {
            kind: "gate_note".into(),
            from_step_run: None,
            from_item: Some("i1".into()),
            from_row: None,
            label: "gate-note:b@1".into(),
            taint: Taint::Clean,
        }]
    );
    let images = reader.row_history_of(TABLE, ROW_1).unwrap();
    assert_eq!(images.len(), 2);
    assert_eq!(
        reader.row_snapshot(TABLE, ROW_1).unwrap(),
        Some(RowSnapshot {
            seq: images[1].seq,
            version: 2,
            data: r#"{"ref":"r-1","state":"done"}"#.into(),
            taint: Taint::Tainted,
        })
    );
    assert_eq!(
        reader
            .row_snapshot(OTHER_TABLE, ROW_2)
            .unwrap()
            .map(|s| (s.version, s.taint)),
        Some((1, Taint::Clean))
    );
    let kinds: Vec<Option<String>> = reader
        .tool_calls("r1-a")
        .unwrap()
        .into_iter()
        .chain(reader.tool_calls("r1-b").unwrap())
        .map(|t| t.result_kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            Some("engine".into()),
            Some("engine".into()),
            Some("untrusted".into()),
            None
        ]
    );
    drop(reader);
    drop(store);

    let at_11 = tempfile::tempdir().unwrap();
    let path_11 = at_11.path().join("state.db");
    am_store::testing::database_at_version(&path_11, BEFORE_ROW_INPUTS + 1).unwrap();
    assert!(columns(&path_11, "step_inputs").contains(&"from_row".to_string()));
    assert!(
        columns(&path_11, "document_versions").is_empty(),
        "no document_versions before 3.6b"
    );
}

/// Row 45: a database already at version 11 opens with nothing applied and
/// no new pre-migration copy.
#[test]
fn reopening_a_version_11_database_applies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_10(dir.path());
    rows_at_10(&path);
    let (store, first) = open_project(dir.path()).unwrap();
    assert!(
        first
            .applied
            .iter()
            .any(|m| (m.from, m.to) == (BEFORE_ROW_INPUTS, BEFORE_ROW_INPUTS + 1)),
        "{first:?}"
    );
    drop(store);
    let before = tables_of(&path);
    let backups = |root: &Path| -> usize { walk_count(&layout(root).dir().join("backups")) };
    let copies = backups(dir.path());

    let (_store, again) = open_project(dir.path()).unwrap();

    assert!(again.applied.is_empty(), "{again:?}");
    assert_eq!(again.version, SCHEMA_VERSION);
    let c = Connection::open(&path).unwrap();
    assert_eq!(user_version(&c), SCHEMA_VERSION);
    drop(c);
    assert_eq!(backups(dir.path()), copies, "no new pre-migration copy");
    let after = tables_of(&path);
    for table in before.keys().filter(|t| *t != "audit_log") {
        assert_eq!(after[table], before[table], "{table}");
    }
}

/// Row 45: the step fails closed and names its backup. A `from_row` column
/// added to `step_inputs` by hand stops 0011; the database is left at
/// version 10 with every row, and the error holds the copy's path.
#[test]
fn a_failing_0011_names_the_backup_and_leaves_version_10() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_10(dir.path());
    rows_at_10(&path);
    Connection::open(&path)
        .unwrap()
        .execute_batch("ALTER TABLE step_inputs ADD COLUMN from_row TEXT;")
        .unwrap();
    let before = tables_of(&path);

    let refused = match open_project(dir.path()) {
        Ok((_, report)) => panic!("the forged column must fail 0011 closed: {report:?}"),
        Err(e) => e,
    };

    let words = refused.to_string();
    match &refused {
        StoreError::MigrationFailed {
            from, to, backup, ..
        } => {
            assert_eq!((*from, *to), (BEFORE_ROW_INPUTS, BEFORE_ROW_INPUTS + 1));
            let backup = backup.as_ref().expect("the step names its backup");
            assert!(backup.is_file(), "the pre-migration copy is kept");
            assert!(
                words.contains(&backup.display().to_string()),
                "the error names the backup: {words}"
            );
        }
        other => panic!("MigrationFailed expected: {other:?}"),
    }
    let c = Connection::open(&path).unwrap();
    assert_eq!(user_version(&c), BEFORE_ROW_INPUTS, "the step rolled back");
    drop(c);
    assert_eq!(tables_of(&path), before, "every row as it was");
}

// ------------------------------- review triage round 1 (manager, 2026-11-25)
//
// R5 (C4, S3): 0011 guards a row edge once journaled, and a raw insert,
// as the writer does: an edge never changes; its label names its own
// after-image; and inserting one taints its attempt (AD-10: reading any row
// taints the reader). R6 (C5, S5): `StartAttempt` refuses an edge whose
// label is not its after-image's. R7 (C6, S4): `StartAttempt` refuses an
// edge that does not name its row's current after-image, as `RowRefused`,
// so the engine can tell a row that moved from any other refusal. Written
// before the fixes.

/// The words the new guards abort with (tester's choice, R5).
const EDGE_FIXED: &str = "a row input never changes";
const EDGE_LABEL: &str = "a row input's label names its own after-image";

/// A raw connection to the rig's database, as the store slice's trigger
/// tests open one.
fn raw(r: &Rig) -> Connection {
    Connection::open(&r.db).unwrap()
}

/// The words a raw statement was aborted with (it must fail).
fn aborted_with(result: rusqlite::Result<usize>, what: &str) -> String {
    match result {
        Ok(n) => panic!("{what}: must abort, changed {n} row(s)"),
        Err(e) => e.to_string(),
    }
}

/// A `row` edge labelled for `row` of `table` at `version`, naming the
/// after-image `from_row`.
fn edge_at(table: &str, row: &str, version: i64, from_row: i64, taint: Taint) -> StepInput {
    StepInput {
        kind: "row".into(),
        from_step_run: None,
        from_item: None,
        from_row: Some(from_row),
        label: format!("row:{table}/{row}@{version}"),
        taint,
    }
}

/// Two rows of `entries`, each holding [`CANARY`]: `dirty`, written by a
/// tainted attempt (version 1), and `fine`, written and then updated by a
/// clean one (versions 1 and 2, both clean); and a later run of `keys`.
struct Rows {
    r: Rig,
    dirty: String,
    fine: String,
    later: Run,
}

impl Rows {
    fn seq(&self, row: &str, version: i64) -> i64 {
        self.r
            .history(row)
            .into_iter()
            .find(|h| h.version == version)
            .unwrap_or_else(|| panic!("no version {version} of {row}"))
            .seq
    }
}

fn rows_and(keys: &[&str]) -> Rows {
    let r = rig_with(&["collect", "check"]);
    let tainted = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Tainted);
    let dirty = r.insert(&tainted, 1, &json!({ "ref": "r-1", "title": CANARY }));
    r.finish(&tainted);
    let clean = r.begin(&r.run, "check", Some(TABLE), None, Taint::Clean);
    let fine = r.insert(&clean, 1, &json!({ "ref": "r-2", "title": CANARY }));
    r.update(&clean, 2, &fine, 1, &json!({ "state": "done" }));
    r.finish(&clean);
    assert_eq!(r.row(&fine).version, 2);
    assert_eq!(r.row(&fine).taint, Taint::Clean);
    let later = r.new_run(keys);
    Rows {
        r,
        dirty,
        fine,
        later,
    }
}

/// R5: a row edge never changes once journaled. A raw `UPDATE` of its
/// taint (lowering it), its kind, its label, its attempt or its time is
/// aborted with the guard's words; one of its after-image is aborted too
/// (by that guard or 3.22's `from_row` guard). And (tester's addition, for
/// the manager to confirm) an `UPDATE` that turns another input into a
/// `row` edge is aborted. Every input stays as it was.
#[test]
fn a_row_edge_never_changes_once_journaled() {
    let x = rows_and(&["read", "other"]);
    let r = &x.r;
    let read = r
        .start_with(
            &x.later,
            "read",
            writes(None, None),
            Taint::Clean,
            vec![edge_at(
                TABLE,
                &x.dirty,
                1,
                x.seq(&x.dirty, 1),
                Taint::Tainted,
            )],
            vec![],
        )
        .unwrap();
    let other = r
        .start_with(
            &x.later,
            "other",
            writes(None, None),
            Taint::Clean,
            vec![StepInput {
                kind: "untrusted".into(),
                from_step_run: None,
                from_item: None,
                from_row: None,
                label: "test:page".into(),
                taint: Taint::Tainted,
            }],
            vec![],
        )
        .unwrap();
    let before = r.state();
    let edges = (
        r.reader().step_inputs(&read.id).unwrap(),
        r.reader().step_inputs(&other.id).unwrap(),
    );
    let c = raw(r);

    for (what, sql) in [
        (
            "lowering its taint",
            "UPDATE step_inputs SET taint = 0 WHERE kind = 'row'".to_string(),
        ),
        (
            "changing its kind",
            "UPDATE step_inputs SET kind = 'untrusted' WHERE kind = 'row'".to_string(),
        ),
        (
            "changing its label",
            format!("UPDATE step_inputs SET label = 'row:{TABLE}/forged@9' WHERE kind = 'row'"),
        ),
        (
            "moving it to another attempt",
            format!(
                "UPDATE step_inputs SET step_run_id = '{}' WHERE kind = 'row'",
                other.id
            ),
        ),
        (
            "changing its time",
            "UPDATE step_inputs SET created_at = '2026-01-01T00:00:00.000Z' WHERE kind = 'row'"
                .to_string(),
        ),
    ] {
        let words = aborted_with(c.execute(&sql, []), what);
        assert!(words.contains(EDGE_FIXED), "{what}: {words}");
    }
    let words = aborted_with(
        c.execute(
            "UPDATE step_inputs SET from_row = NULL WHERE kind = 'row'",
            [],
        ),
        "clearing its after-image",
    );
    assert!(
        words.contains(EDGE_FIXED) || words.contains("after-image never changes"),
        "{words}"
    );
    aborted_with(
        c.execute(
            "UPDATE step_inputs SET kind = 'row' WHERE kind = 'untrusted'",
            [],
        ),
        "turning another input into a row edge",
    );
    drop(c);
    assert_eq!(r.state(), before, "every input as it was");
    assert_eq!(
        (
            r.reader().step_inputs(&read.id).unwrap(),
            r.reader().step_inputs(&other.id).unwrap(),
        ),
        edges
    );
}

/// R5: a raw insert of a row edge whose label is not its after-image's is
/// aborted with the guard's words: the version, the row or the table
/// another than the image's, or no row label at all. Nothing is written,
/// and the attempt's taint is as it was.
#[test]
fn a_raw_row_edge_must_carry_its_after_images_label() {
    let x = rows_and(&["read"]);
    let r = &x.r;
    let read = r.latest(&x.later, "read");
    let image = x.seq(&x.fine, 2);
    let before = r.state();
    let c = raw(r);

    for (what, label) in [
        ("another version", format!("row:{TABLE}/{}@1", x.fine)),
        ("another row", format!("row:{TABLE}/{}@2", x.dirty)),
        ("another table", format!("row:{OTHER_TABLE}/{}@2", x.fine)),
        ("no row label", "test:page".to_string()),
    ] {
        let words = aborted_with(
            c.execute(
                "INSERT INTO step_inputs (step_run_id, kind, from_step_run, from_item, from_row, label, taint, created_at) VALUES (?1, 'row', NULL, NULL, ?2, ?3, 0, '2026-11-25T10:00:00.000Z')",
                rusqlite::params![read.id, image, label],
            ),
            what,
        );
        assert!(words.contains(EDGE_LABEL), "{what}: {words}");
    }
    drop(c);
    assert_eq!(r.state(), before, "nothing written");
    assert_eq!(r.step(&read.id).taint, Taint::Clean);
}

/// R5 (AD-10: reading any row taints the reader): a row edge inserted by
/// hand with its own image's label, a clean one, raises its attempt's
/// `step_runs.taint` to 1, as the writer does.
#[test]
fn a_raw_row_edge_taints_its_attempt() {
    let x = rows_and(&["read"]);
    let r = &x.r;
    let read = r.latest(&x.later, "read");
    assert_eq!(read.taint, Taint::Clean);

    raw(r)
        .execute(
            "INSERT INTO step_inputs (step_run_id, kind, from_step_run, from_item, from_row, label, taint, created_at) VALUES (?1, 'row', NULL, NULL, ?2, ?3, 0, '2026-11-25T10:00:00.000Z')",
            rusqlite::params![
                read.id,
                x.seq(&x.fine, 2),
                format!("row:{TABLE}/{}@2", x.fine)
            ],
        )
        .unwrap();

    assert_eq!(
        r.step(&read.id).taint,
        Taint::Tainted,
        "the reader is tainted"
    );
    assert_eq!(r.row(&x.fine).taint, Taint::Clean, "the row's mark is not");
}

/// R6: `StartAttempt` refuses a row edge whose label is not the
/// after-image's own, `row:<table>/<row id>@<version>`: another version,
/// another row, another table, or no row label. Nothing is written, the
/// attempt stays queued, and the words hold no row data. The control: the
/// image's own label starts it.
#[test]
fn a_start_whose_row_edge_is_mislabelled_is_refused() {
    let x = rows_and(&["read"]);
    let r = &x.r;
    let image = x.seq(&x.fine, 2);
    let read = r.latest(&x.later, "read");

    for (what, input) in [
        (
            "another version",
            edge_at(TABLE, &x.fine, 1, image, Taint::Clean),
        ),
        (
            "another row",
            edge_at(TABLE, &x.dirty, 2, image, Taint::Clean),
        ),
        (
            "another table",
            edge_at(OTHER_TABLE, &x.fine, 2, image, Taint::Clean),
        ),
        (
            "no row label",
            StepInput {
                label: "test:page".into(),
                ..edge_at(TABLE, &x.fine, 2, image, Taint::Clean)
            },
        ),
    ] {
        let before = r.state();
        let refused = r
            .start_with(
                &x.later,
                "read",
                writes(None, None),
                Taint::Clean,
                vec![input],
                vec![],
            )
            .expect_err(what);
        let words = refused.to_string();
        assert!(!words.contains(CANARY), "{what}: {words}");
        assert_eq!(r.state(), before, "{what}: nothing written");
        assert_eq!(r.step(&read.id).status, "queued", "{what}");
    }

    r.start_with(
        &x.later,
        "read",
        writes(None, None),
        Taint::Clean,
        vec![edge_at(TABLE, &x.fine, 2, image, Taint::Clean)],
        vec![],
    )
    .unwrap();
}

/// R7 (AD-8, AD-43: lineage names the image that was sent, current when
/// the start commits): `StartAttempt` refuses a row edge that names an
/// older after-image of its row, though its label is that image's own:
/// `StoreError::RowRefused` (tester's choice, so the engine tells a row
/// that moved from any other refusal), in words that name the row id and
/// hold no row data; nothing is written and the attempt stays queued. The
/// control: the current image starts it.
#[test]
fn a_start_whose_row_edge_names_an_older_image_is_refused() {
    let x = rows_and(&["read"]);
    let r = &x.r;
    let read = r.latest(&x.later, "read");
    let before = r.state();

    let refused = r
        .start_with(
            &x.later,
            "read",
            writes(None, None),
            Taint::Clean,
            vec![edge_at(TABLE, &x.fine, 1, x.seq(&x.fine, 1), Taint::Clean)],
            vec![],
        )
        .expect_err("version 1 is not the row's current image");

    match &refused {
        StoreError::RowRefused(words) => {
            assert!(words.contains(&x.fine), "names the row: {words}");
            assert!(!words.contains(CANARY), "no row data: {words}");
        }
        other => panic!("RowRefused expected: {other:?}"),
    }
    assert_eq!(r.state(), before, "nothing written");
    assert_eq!(r.step(&read.id).status, "queued");

    r.start_with(
        &x.later,
        "read",
        writes(None, None),
        Taint::Clean,
        vec![edge_at(TABLE, &x.fine, 2, x.seq(&x.fine, 2), Taint::Clean)],
        vec![],
    )
    .unwrap();
}

// ------------------------------- review triage round 2 (manager, 2026-11-25)

/// The words of T4's guard (tester's choice).
const NEVER_REPLACED: &str = "a step input is never replaced";

/// Review triage round 2, T4 (F4; the 3.5 lesson): an `INSERT OR REPLACE`
/// into `step_inputs` by an existing row edge's id would delete the edge
/// without an `UPDATE` and put another input in its place. It is aborted
/// with the guard's words, with SQLite's `recursive_triggers` off (a plain
/// connection) and on (as the store opens its own): replaced by a clean
/// plan input, by a clean `gate_note`, or by the very same edge. Every
/// input stays as it was.
#[test]
fn a_row_edge_is_never_replaced_by_its_id() {
    let x = rows_and(&["read"]);
    let r = &x.r;
    let read = r
        .start_with(
            &x.later,
            "read",
            writes(None, None),
            Taint::Clean,
            vec![edge_at(
                TABLE,
                &x.dirty,
                1,
                x.seq(&x.dirty, 1),
                Taint::Tainted,
            )],
            vec![],
        )
        .unwrap();
    let before = r.state();
    let edges = r.reader().step_inputs(&read.id).unwrap();
    let id: i64 = raw(r)
        .query_row("SELECT id FROM step_inputs WHERE kind = 'row'", [], |row| {
            row.get(0)
        })
        .unwrap();

    for recursive in [false, true] {
        let c = raw(r);
        c.pragma_update(None, "recursive_triggers", recursive)
            .unwrap();
        for (what, sql) in [
            (
                "by a clean plan input",
                "INSERT OR REPLACE INTO step_inputs (id, step_run_id, kind, from_step_run, from_item, from_row, label, taint, created_at) SELECT id, step_run_id, 'untrusted', NULL, NULL, NULL, 'test:page', 0, created_at FROM step_inputs WHERE id = ?1",
            ),
            (
                "by a clean gate note",
                "INSERT OR REPLACE INTO step_inputs (id, step_run_id, kind, from_step_run, from_item, from_row, label, taint, created_at) SELECT id, step_run_id, 'gate_note', NULL, NULL, NULL, 'gate-note:x@1', 0, created_at FROM step_inputs WHERE id = ?1",
            ),
            (
                "by the very same edge",
                "INSERT OR REPLACE INTO step_inputs SELECT * FROM step_inputs WHERE id = ?1",
            ),
        ] {
            let what = format!("{what} (recursive_triggers {recursive})");
            let words = aborted_with(c.execute(sql, rusqlite::params![id]), &what);
            assert!(words.contains(NEVER_REPLACED), "{what}: {words}");
        }
        drop(c);
        assert_eq!(r.state(), before, "recursive_triggers {recursive}");
        assert_eq!(r.reader().step_inputs(&read.id).unwrap(), edges);
        assert_eq!(r.step(&read.id).taint, Taint::Tainted);
    }
}
