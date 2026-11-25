//! The store slice of epic 3 story 6, "Row store and history" (AD-7,
//! AD-8, AD-10 as amended and restated 2026-11-24, AD-21, AD-24, AD-25,
//! AD-35): the store holds rows with provenance, and nothing reads or
//! writes them for a step yet.
//!
//! - Migration 0010 adds `table_rows` (one row per current row: table id,
//!   row id, JSON data, version, taint), the append-only `row_history` of
//!   after-images (Actor, run, step run, seat, call sequence, tool call),
//!   what an attempt may write on `step_runs` (`seat`, `writes_table`) and a
//!   result's kind on its tool call's journal row (`tool_calls.result_kind`,
//!   `.result_label`; AD-8, AD-10 with Winston's wording). It runs behind
//!   AD-7's pre-migration copy and keeps every old row value for value. It
//!   adds no `from_row` (3.6c) and no `document_versions` (3.6b).
//! - `Writer::apply_row_write(actor, RowWrite)` is the one row command (not
//!   a `WriteCommand`): a Seat of the attempt, a `running` attempt, an
//!   `issued` `table_write` call at the key, the attempt's journaled table;
//!   insert, or update at the current version; the after-image, the row and
//!   the call's `succeeded` with its result kind `engine` in one
//!   transaction; a repeated key writes nothing. A stale or unknown row is
//!   `StoreError::RowRefused` in words, with nothing changed (owner Q2).
//! - Each row stores its own taint on write, the highest of the attempt's
//!   committed taint, the write's and the row's; it only rises, and each
//!   after-image records it. `FinishToolCall` journals `untrusted`.
//! - The read path is 3.6c's (`tests/row_inputs.rs`); documents are 3.6b's
//!   (`tests/documents.rs`). Audit entries hold ids, versions and counts, never row data; the
//!   receipt a result is rebuilt from holds no data either.
//!
//! The pinned API is in the store plan's Implementation Notes, "API the
//! tests fix (store slice)". Written before migration 0010 or any of it
//! exists.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use am_domain::rows::{RowOp, RowWrite};
use am_domain::{Actor, REVIEWER_HAT, RuntimeLayout, Taint};
use am_store::audit::sha256_hex;
use am_store::{
    AttemptWrites, CallPurpose, CallRow, InboxItemRow, NewPromptPart, OpenReport, Reader,
    RowHistoryRecord, RowRecord, SCHEMA_VERSION, StepInput, StepRow, Store, StoreError,
    ToolCallRow, WriteCommand, new_id,
};
use rusqlite::types::Value as Cell;
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};
use uuid::Uuid;

const HASH: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";

/// The table every writing attempt declares, and another one.
const TABLE: &str = "entries";
const OTHER_TABLE: &str = "others";
/// The seat every attempt runs on.
const SEAT: &str = "seat-writer";

/// Canaries no audit entry may hold (AD-25).
const CANARY_DATA: &str = "canary-row-data-5d1f07";
const CANARY_PATCH: &str = "canary-row-patch-8b42e9";

/// The schema version before migration 0010.
const BEFORE_ROWS: i64 = 9;

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

/// The owner as an `Actor` (a person, never a row's writer).
fn you_reviewer() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: REVIEWER_HAT.into(),
    }
}

/// A taint flag as an audit argument: `1` or `true` (the writer's choice).
fn audited_tainted(arg: &Value) -> bool {
    arg == &Value::from(1) || arg == &Value::Bool(true)
}

/// A clean flag as an audit argument: `0` or `false`.
fn audited_clean(arg: &Value) -> bool {
    arg == &Value::from(0) || arg == &Value::Bool(false)
}

/// A store timestamp: UTC RFC 3339 with milliseconds and `Z`.
fn assert_timestamp(text: &str) {
    assert_eq!(text.len(), "2026-11-24T10:00:00.000Z".len(), "{text}");
    assert!(text.ends_with('Z') && text.as_bytes()[19] == b'.', "{text}");
    text.parse::<jiff::Timestamp>()
        .unwrap_or_else(|e| panic!("{text} is not a timestamp: {e}"));
}

fn json_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

fn insert_op(row_id: Uuid, data: &Value) -> RowOp {
    RowOp::Insert {
        row_id,
        data_json: data.to_string(),
    }
}

fn update_op(row_id: Uuid, expected_version: i64, patch: &Value) -> RowOp {
    RowOp::Update {
        row_id,
        expected_version,
        patch_json: patch.to_string(),
    }
}

fn writes(table: Option<&str>, document: Option<&str>) -> AttemptWrites {
    AttemptWrites {
        seat: SEAT.into(),
        table: table.map(str::to_string),
        document: document.map(str::to_string),
    }
}

// ------------------------------------------------------------------ the rig

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
    /// Every row of `row_history`, also one whose row does not exist.
    history_rows: i64,
    steps: Vec<StepRow>,
    calls: Vec<CallRow>,
    tool_calls: Vec<ToolCallRow>,
    items: Vec<InboxItemRow>,
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

/// A run of `collect`, `check` and `plain`.
fn rig() -> Rig {
    rig_with(&["collect", "check", "plain"])
}

impl Rig {
    fn reader(&self) -> Reader {
        self.store.reader().unwrap()
    }

    fn exec(&self, cmd: WriteCommand) -> Result<(), StoreError> {
        self.store.writer().exec(engine(), cmd)
    }

    fn apply(&self, actor: Actor, write: RowWrite) -> Result<(), StoreError> {
        self.store.writer().apply_row_write(actor, write)
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

    /// Starts the latest attempt of `key`, which may write `table` and
    /// version `document`.
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

    /// A tool call of `tool` journaled with `status` at `seq`: its id.
    fn record(&self, a: &Attempt, seq: i64, tool: &str, status: &str) -> String {
        let id = new_id();
        self.exec(WriteCommand::RecordToolCall {
            tool_call_id: id.clone(),
            step_run_id: a.id.clone(),
            call_seq: seq,
            tool: tool.into(),
            status: status.into(),
        })
        .unwrap();
        id
    }

    /// A `table_write` call journaled `issued` at `seq`: its id.
    fn issue(&self, a: &Attempt, seq: i64) -> String {
        self.record(a, seq, "table_write", "issued")
    }

    /// `write` for the call `call` at `seq` of `a`, on `entries`, as the
    /// attempt's seat, with a clean command taint.
    fn write_at(&self, a: &Attempt, seq: i64, call: &str, op: RowOp) -> Result<(), StoreError> {
        self.apply(a.actor(), row_write(a, seq, call, TABLE, op, Taint::Clean))
    }

    /// Issues call `seq` and inserts a new row holding `data`: its id.
    fn insert(&self, a: &Attempt, seq: i64, data: &Value) -> String {
        let call = self.issue(a, seq);
        let row = Uuid::now_v7();
        self.write_at(a, seq, &call, insert_op(row, data)).unwrap();
        row.to_string()
    }

    /// Issues call `seq` and updates `row` read at `expected`.
    fn update(
        &self,
        a: &Attempt,
        seq: i64,
        row: &str,
        expected: i64,
        patch: &Value,
    ) -> Result<(), StoreError> {
        let call = self.issue(a, seq);
        self.write_at(
            a,
            seq,
            &call,
            update_op(Uuid::parse_str(row).unwrap(), expected, patch),
        )
    }

    /// Reserves the attempt's next model call: its id.
    fn reserve(&self, a: &Attempt) -> String {
        let call = new_id();
        let seq = self.reader().calls_for_step(&a.id).unwrap().len() as i64 + 1;
        self.exec(WriteCommand::ReserveCall {
            step_run_id: a.id.clone(),
            call_id: call.clone(),
            call_seq: seq,
            reserved_micro_usd: 0,
            rate_snapshot: "none".into(),
            taint: Taint::Clean,
            purpose: CallPurpose::Turn,
            prompt: vec![],
        })
        .unwrap();
        call
    }

    /// The attempt's call settled and the attempt `succeeded`.
    fn finish(&self, a: &Attempt) {
        let call = self.reserve(a);
        self.exec(WriteCommand::SettleAndFinishStep {
            call_id: call,
            usage: None,
            cost_micro_usd: None,
            step_run_id: a.id.clone(),
            status: "succeeded".into(),
            output: Some("done".into()),
        })
        .unwrap();
    }

    /// Step `from` of `run` runs and loops back to `to`: attempt n+1 of
    /// every step from `to` through `from` is queued.
    fn loop_back(&self, run: &Run, from: &str, to: &str) {
        let a = self.begin(run, from, None, None, Taint::Clean);
        let call = self.reserve(&a);
        self.exec(WriteCommand::SettleAndLoopBack {
            call_id: call,
            usage: None,
            cost_micro_usd: None,
            step_run_id: a.id,
            output: r#"{"reasons":["once more"]}"#.into(),
            to: to.into(),
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

    fn tool_call(&self, a: &Attempt, call: &str) -> ToolCallRow {
        self.reader()
            .tool_calls(&a.id)
            .unwrap()
            .into_iter()
            .find(|t| t.id == call)
            .unwrap_or_else(|| panic!("no tool call {call}"))
    }

    fn audit(&self) -> Vec<Value> {
        let mut out = Vec::new();
        self.reader().export_audit_jsonl(&mut out).unwrap();
        String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn entries(&self, action: &str) -> Vec<Value> {
        self.audit()
            .into_iter()
            .filter(|e| e["action"] == action)
            .collect()
    }

    fn raw(&self) -> Connection {
        Connection::open(&self.db).unwrap()
    }

    fn count(&self, table: &str) -> i64 {
        let c = Connection::open_with_flags(&self.db, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn state(&self) -> State {
        let reader = self.reader();
        let mut rows = reader.table_rows_of(TABLE).unwrap();
        rows.extend(reader.table_rows_of(OTHER_TABLE).unwrap());
        let history = rows
            .iter()
            .flat_map(|r| reader.row_history_of(&r.table_id, &r.row_id).unwrap())
            .collect();
        let (mut steps, mut calls, mut tool_calls, mut items) = (vec![], vec![], vec![], vec![]);
        for run in reader.runs().unwrap() {
            for s in reader.steps(&run.id).unwrap() {
                tool_calls.extend(reader.tool_calls(&s.id).unwrap());
                steps.push(s);
            }
            calls.extend(reader.calls_for_run(&run.id).unwrap());
            items.extend(reader.inbox_items(&run.id).unwrap());
        }
        State {
            rows,
            history,
            history_rows: self.count("row_history"),
            steps,
            calls,
            tool_calls,
            items,
            audit: self.audit().len(),
        }
    }
}

fn row_write(a: &Attempt, seq: i64, call: &str, table: &str, op: RowOp, taint: Taint) -> RowWrite {
    RowWrite {
        tool_call_id: call.into(),
        step_run_id: a.id.clone(),
        call_seq: seq,
        table_id: table.into(),
        op,
        taint,
    }
}

// ------------------------------------------------------------ migration 0010

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

/// A schema-9 database in the project's runtime folder, made by hand.
fn database_at_9(root: &Path) -> PathBuf {
    existing_runtime(root);
    let path = layout(root).state_db();
    am_store::testing::database_at_version(&path, BEFORE_ROWS).unwrap();
    path
}

/// Rows in every table a run touches, as 3.3's to 3.5's writers made them:
/// two runs (one running with a tainted attempt, one failed), attempts,
/// settled and open calls (a turn and a repair) with their ledger rows,
/// inputs (a step output, a plan input, a gate note), a tool call, an open
/// Review and a dismissed Failure item, prompt parts, a trigger and a fire.
fn rows_at_9(path: &Path) {
    let step = "id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at";
    let call = "id, step_run_id, call_seq, status, settled, input_tokens, output_tokens, created_at, purpose";
    let part =
        "step_run_id, call_seq, ord, slot, kind, label, taint, text, bytes, sha256, created_at";
    Connection::open(path)
        .unwrap()
        .execute_batch(&format!(
            "INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
               VALUES ('r1', 'wf', 'manual', 'running', '2026-11-20T10:00:00.000Z', '2026-11-20T10:00:09.000Z', '{HASH}', NULL);
             INSERT INTO runs (id, workflow, origin, status, created_at, updated_at, config_hash, finished_at)
               VALUES ('r2', 'wf', 'schedule', 'failed', '2026-11-20T11:00:00.000Z', '2026-11-20T11:00:09.000Z', NULL, '2026-11-20T11:00:09.000Z');
             INSERT INTO step_runs ({step})
               VALUES ('r1-a', 'r1', 'a', 0, 1, 'succeeded', 'out of a', 0, '2026-11-20T10:00:03.000Z', '2026-11-20T10:00:01.000Z', '2026-11-20T10:00:03.000Z');
             INSERT INTO step_runs ({step})
               VALUES ('r1-b', 'r1', 'b', 1, 1, 'running', NULL, 1, '2026-11-20T10:00:09.000Z', '2026-11-20T10:00:04.000Z', NULL);
             INSERT INTO step_runs ({step})
               VALUES ('r1-c', 'r1', 'c', 2, 1, 'queued', NULL, 0, '2026-11-20T10:00:00.000Z', NULL, NULL);
             INSERT INTO step_runs ({step})
               VALUES ('r2-a', 'r2', 'a', 0, 1, 'failed', NULL, 0, '2026-11-20T11:00:09.000Z', '2026-11-20T11:00:01.000Z', '2026-11-20T11:00:09.000Z');
             INSERT INTO model_calls ({call})
               VALUES ('c1', 'r1-a', 1, 'settled', 1, 12, 7, '2026-11-20T10:00:02.000Z', 'turn');
             INSERT INTO model_calls ({call})
               VALUES ('c4', 'r1-a', 2, 'settled', 1, 9, 4, '2026-11-20T10:00:02.500Z', 'repair');
             INSERT INTO model_calls ({call})
               VALUES ('c2', 'r1-b', 1, 'reserved', 0, NULL, NULL, '2026-11-20T10:00:05.000Z', 'turn');
             INSERT INTO model_calls ({call})
               VALUES ('c3', 'r2-a', 1, 'settled', 1, 3, 0, '2026-11-20T11:00:02.000Z', 'turn');
             INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at)
               VALUES ('l1', 'c1', 'r1', 'model_call', 0, 0, 'settled', 'none', '2026-11-20T10:00:02.000Z');
             INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at)
               VALUES ('l2', 'c2', 'r1', 'model_call', 0, NULL, 'reserved', 'none', '2026-11-20T10:00:05.000Z');
             INSERT INTO ledger (id, model_call_id, run_id, kind, reserved_micro_usd, settled_micro_usd, state, rate_snapshot, created_at)
               VALUES ('l3', 'c3', 'r2', 'model_call', 0, 0, 'settled', 'none', '2026-11-20T11:00:02.000Z');
             INSERT INTO step_inputs (step_run_id, kind, from_step_run, label, taint, created_at, from_item)
               VALUES ('r1-b', 'step_output', 'r1-a', 'step:a@1', 0, '2026-11-20T10:00:04.000Z', NULL);
             INSERT INTO step_inputs (step_run_id, kind, from_step_run, label, taint, created_at, from_item)
               VALUES ('r1-b', 'untrusted', NULL, 'test:page', 1, '2026-11-20T10:00:04.000Z', NULL);
             INSERT INTO tool_calls (id, step_run_id, call_seq, tool, status, created_at, finished_at)
               VALUES ('t1', 'r1-b', 1, 'fixture_read', 'succeeded', '2026-11-20T10:00:06.000Z', '2026-11-20T10:00:07.000Z');
             INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, max_returns)
               VALUES ('i1', 'review', 'open', 'r1', 'r1-b', 'r1-a', 1, 'Review', 'Step b waits for your review.', '2026-11-20T10:00:09.000Z', 2);
             INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, resolved_at, resolved_by, decision)
               VALUES ('i2', 'failure', 'resolved', 'r2', 'r2-a', 'r2-a', 0, 'A step stopped', 'Step a stopped.', '2026-11-20T11:00:09.000Z', '2026-11-20T11:05:00.000Z', '{{\"kind\":\"human\",\"person\":\"owner\",\"hat\":\"Reviewer\"}}', 'dismiss');
             INSERT INTO step_inputs (step_run_id, kind, from_step_run, label, taint, created_at, from_item)
               VALUES ('r1-c', 'gate_note', NULL, 'gate-note:b@1', 0, '2026-11-20T10:00:10.000Z', 'i1');
             INSERT INTO prompt_parts ({part})
               VALUES ('r1-b', 0, 1, 'step', 'instruction', 'Step instruction', 0, 'Do the work of b.', 17, NULL, '2026-11-20T10:00:04.000Z');
             INSERT INTO prompt_parts ({part})
               VALUES ('r1-b', 0, 2, 'untrusted', 'untrusted', 'test:page', 1, 'a page', 6, NULL, '2026-11-20T10:00:04.000Z');
             INSERT INTO triggers (id, cron, tz, catch_up, state, next_fire, last_fired)
               VALUES ('trg', '0 9 * * *', 'UTC', 'once', 'active', '2026-11-21T09:00:00.000Z', NULL);
             INSERT INTO fires (trigger_id, scheduled_for, fired_at, kind)
               VALUES ('trg', '2026-11-20T09:00:00.000Z', '2026-11-20T09:00:01.000Z', 'on_time');"
        ))
        .unwrap();
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
        out.insert(name.clone(), (cols.clone(), rows_of(&conn, &name, &cols)));
    }
    out
}

fn rows_of(conn: &Connection, table: &str, cols: &[String]) -> Vec<Vec<Cell>> {
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

/// Row 32: every row of every table the schema-9 database held is the same
/// value for value after 0010 (in its old columns; the two logs a migration
/// appends to keep their old rows as a prefix); the pre-migration copy the
/// report names exists, opens at version 9 and holds every row as it was,
/// also after a reopen; the step is reported 9 to 10 and audited.
#[test]
fn migration_0010_keeps_every_row_and_the_pre_migration_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_9(dir.path());
    rows_at_9(&path);
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
        "triggers",
        "fires",
    ] {
        assert!(!before[table].1.is_empty(), "{table} holds rows to keep");
    }
    for table in ["table_rows", "row_history"] {
        assert!(!before.contains_key(table), "{table} is 0010's");
    }

    let (store, report) = open_project(dir.path()).unwrap();

    // Later slices add 0011 and 0012 after it: the 9 to 10 step is applied
    // on the way, not necessarily last.
    const { assert!(SCHEMA_VERSION > BEFORE_ROWS) };
    assert_eq!(report.version, SCHEMA_VERSION);
    let step = report
        .applied
        .iter()
        .find(|m| m.from == BEFORE_ROWS)
        .expect("the 9 to 10 step is reported");
    assert_eq!(step.to, BEFORE_ROWS + 1);
    let backup = step.backup.clone().expect("a pre-migration copy (AD-7)");
    assert!(backup.is_file(), "{}", backup.display());
    assert!(
        report
            .lines()
            .iter()
            .any(|l| l.contains("from version 9 to 10")),
        "{:?}",
        report.lines()
    );

    let after = tables_of(&path);
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    for (table, (cols, rows)) in &before {
        let (_, now) = after
            .get(table)
            .unwrap_or_else(|| panic!("{table} was dropped"));
        let old_columns_now = rows_of(&conn, table, cols);
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
        assert_eq!(user_version(&copy), BEFORE_ROWS, "{what}");
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
                .is_some_and(|o| o.starts_with("from version 9 to 10"))),
        "{all:#?}"
    );
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
    drop(reader);
    drop(store);
    let (_store, again) = open_project(dir.path()).unwrap();
    assert!(again.applied.is_empty(), "reopening applies nothing");
    copy_is_as_it_was("after a reopen");
}

/// Row 32: `table_rows` and `row_history` exist with the plan's columns and
/// are empty; the new columns (`step_runs.seat`, `.writes_table`,
/// `tool_calls.result_kind`, `.result_label`) are NULL on every old row, and
/// the readers say so (no writes for an old attempt, no row, no kind on an
/// old tool call). A database made at exactly version 10 has no
/// `step_inputs.from_row` (3.6c's 0011) and no `document_versions` (3.6b's
/// 0012).
#[test]
fn migration_0010_adds_empty_tables_and_null_columns() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_9(dir.path());
    rows_at_9(&path);

    let (store, _) = open_project(dir.path()).unwrap();

    for (table, wanted) in [
        (
            "table_rows",
            &[
                "table_id",
                "row_id",
                "data",
                "version",
                "taint",
                "created_at",
                "updated_at",
            ][..],
        ),
        (
            "row_history",
            &[
                "seq",
                "table_id",
                "row_id",
                "version",
                "data",
                "taint",
                "actor",
                "run_id",
                "step_run_id",
                "seat",
                "call_seq",
                "tool_call_id",
                "op",
                "created_at",
            ][..],
        ),
        ("step_runs", &["seat", "writes_table"][..]),
        ("tool_calls", &["result_kind", "result_label"][..]),
    ] {
        let have = columns(&path, table);
        for column in wanted {
            assert!(have.contains(&column.to_string()), "{table}.{column}");
        }
    }

    let c = Connection::open(&path).unwrap();
    for table in ["table_rows", "row_history"] {
        let n: i64 = c
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "{table} is empty");
    }
    let set: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM step_runs WHERE seat IS NOT NULL OR writes_table IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(set, 0, "no old attempt journaled writes");
    let kinds: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM tool_calls WHERE result_kind IS NOT NULL OR result_label IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kinds, 0, "no old tool call has a result kind");
    drop(c);

    let reader = store.reader().unwrap();
    for id in ["r1-a", "r1-b", "r1-c", "r2-a"] {
        assert_eq!(reader.attempt_writes(id).unwrap(), None, "{id}");
    }
    let old = reader.tool_calls("r1-b").unwrap();
    assert_eq!(old.len(), 1);
    assert_eq!(
        (old[0].result_kind.clone(), old[0].result_label.clone()),
        (None, None),
        "the old call as it was"
    );
    assert!(reader.table_rows_of(TABLE).unwrap().is_empty());
    assert_eq!(reader.row_receipt("r1-b", 1).unwrap(), None);
    assert_eq!(
        reader.step_inputs("r1-b").unwrap().len(),
        2,
        "the old inputs are read as before"
    );
    drop(reader);
    drop(store);

    // Exactly at version 10: no read path and no documents yet.
    let at_10 = tempfile::tempdir().unwrap();
    let path_10 = at_10.path().join("state.db");
    am_store::testing::database_at_version(&path_10, BEFORE_ROWS + 1).unwrap();
    assert!(
        !columns(&path_10, "step_inputs").contains(&"from_row".to_string()),
        "no from_row before 3.6c"
    );
    assert!(
        columns(&path_10, "document_versions").is_empty(),
        "no document_versions before 3.6b"
    );
    assert!(columns(&path_10, "table_rows").contains(&"taint".to_string()));
}

/// Row 32: a database already at version 10 opens with nothing applied and
/// no new pre-migration copy.
#[test]
fn reopening_a_version_10_database_applies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_9(dir.path());
    rows_at_9(&path);
    let (store, first) = open_project(dir.path()).unwrap();
    assert!(
        first
            .applied
            .iter()
            .any(|m| (m.from, m.to) == (BEFORE_ROWS, BEFORE_ROWS + 1)),
        "{first:?}"
    );
    drop(store);
    let before = tables_of(&path);
    let backups = |root: &Path| -> usize {
        let dir = layout(root).dir().join("backups");
        walk_count(&dir)
    };
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

/// Row 32: the step fails closed and names its backup. A table of the
/// name `table_rows` made by hand stops 0010; the database is left at
/// version 9 with every row, and the error holds the copy's path.
#[test]
fn a_failing_0010_names_the_backup_and_leaves_version_9() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_9(dir.path());
    rows_at_9(&path);
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE table_rows (forged TEXT);")
        .unwrap();
    let before = tables_of(&path);

    let refused = match open_project(dir.path()) {
        Ok((_, report)) => panic!("the forged table must fail 0010 closed: {report:?}"),
        Err(e) => e,
    };

    let words = refused.to_string();
    match &refused {
        StoreError::MigrationFailed {
            from, to, backup, ..
        } => {
            assert_eq!((*from, *to), (BEFORE_ROWS, BEFORE_ROWS + 1));
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
    assert_eq!(user_version(&c), BEFORE_ROWS, "the step rolled back");
    drop(c);
    assert_eq!(tables_of(&path), before, "every row as it was");
}

// ------------------------------------------------------------ the row writer

/// Row 2: an insert by the attempt's seat, for its `issued` `table_write`
/// call, writes one row (version 1, the data as sent, the attempt's taint)
/// and its after-image (the Seat actor, the run, the step run, the seat,
/// the call sequence, the tool call, `insert`), finishes the call
/// `succeeded`, journals the attempt's writes, and audits `row.written`
/// with ids only.
#[test]
fn an_insert_writes_the_row_and_its_after_image_together() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    assert_eq!(
        r.reader().attempt_writes(&a.id).unwrap(),
        Some(writes(Some(TABLE), None)),
        "the attempt's writes are journaled at its start"
    );
    let data = json!({ "ref": "r-1", "title": "first", "amount": 2.5 });
    let call = r.issue(&a, 1);
    let row = Uuid::now_v7();

    r.write_at(&a, 1, &call, insert_op(row, &data)).unwrap();

    let id = row.to_string();
    let stored = r.row(&id);
    assert_eq!(
        (stored.table_id.as_str(), stored.row_id.as_str()),
        (TABLE, id.as_str())
    );
    assert_eq!(json_of(&stored.data), data);
    assert_eq!((stored.version, stored.taint), (1, Taint::Clean));
    assert_timestamp(&stored.created_at);
    assert_timestamp(&stored.updated_at);
    assert_eq!(
        r.reader().table_rows_of(TABLE).unwrap(),
        vec![stored.clone()]
    );

    let history = r.history(&id);
    assert_eq!(history.len(), 1, "{history:#?}");
    let image = &history[0];
    assert_eq!(
        (
            image.table_id.as_str(),
            image.row_id.as_str(),
            image.version
        ),
        (TABLE, id.as_str(), 1)
    );
    assert_eq!(json_of(&image.data), data);
    assert_eq!(image.taint, Taint::Clean);
    assert_eq!(image.actor, a.actor());
    assert_eq!(image.run_id.as_deref(), Some(r.run.id.as_str()));
    assert_eq!(image.step_run_id.as_deref(), Some(a.id.as_str()));
    assert_eq!(image.seat.as_deref(), Some(SEAT));
    assert_eq!(image.call_seq, Some(1));
    assert_eq!(image.tool_call_id.as_deref(), Some(call.as_str()));
    assert_eq!(image.op, "insert");
    assert_timestamp(&image.created_at);
    assert_eq!(
        r.reader().row_written_at(&a.id, 1).unwrap().as_ref(),
        Some(image),
        "the after-image is read back by its key"
    );

    let finished = r.tool_call(&a, &call);
    assert_eq!(finished.status, "succeeded");
    assert_eq!(
        (
            finished.result_kind.as_deref(),
            finished.result_label.as_deref()
        ),
        (Some("engine"), Some("tool:table_write")),
        "the result's kind on its tool call's journal row (AD-8, AD-10)"
    );
    let receipt = r
        .reader()
        .row_receipt(&a.id, 1)
        .unwrap()
        .expect("the receipt at the key");
    assert_eq!(
        (
            receipt.row_id.as_str(),
            receipt.version,
            receipt.op.as_str()
        ),
        (id.as_str(), 1, "insert")
    );
    assert_eq!(r.step(&a.id).status, "running", "the step goes on");
    let written = r.entries("row.written");
    assert_eq!(written.len(), 1, "{written:#?}");
    let e = &written[0];
    assert_eq!(e["target"], format!("row:{TABLE}/{id}"));
    assert_eq!(e["args"]["op"], "insert");
    assert_eq!(e["args"]["version"], 1);
    assert_eq!(e["args"]["call_seq"], 1);
    assert_eq!(e["args"]["step_run_id"], a.id.as_str());
    assert!(audited_clean(&e["args"]["taint"]), "{e}");
    assert!(!e["args"]["columns"].is_null(), "a column count: {e}");
    let text = e.to_string();
    assert!(!text.contains("first") && !text.contains("2.5"), "{text}");
}

/// Row 3: an update names the version it read; version 2 is version 1 with
/// the named columns merged (`null` clears one), its after-image holds the
/// whole row, version 1's after-image is unchanged, the row keeps its
/// creation time, and the audit says `update` at version 2.
#[test]
fn an_update_needs_the_current_version_and_appends_an_after_image() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let id = r.insert(
        &a,
        1,
        &json!({ "ref": "r-1", "title": "first", "amount": 2 }),
    );
    let first = r.history(&id);
    let created = r.row(&id).created_at;

    r.update(&a, 2, &id, 1, &json!({ "title": "second", "amount": null }))
        .unwrap();

    let row = r.row(&id);
    assert_eq!(row.version, 2);
    let data = json_of(&row.data);
    assert_eq!(data["ref"], "r-1", "a column not named is kept");
    assert_eq!(data["title"], "second");
    assert!(
        data.get("amount").is_none_or(Value::is_null),
        "`null` clears the column: {data}"
    );
    assert_eq!(row.created_at, created, "created once");
    assert!(row.updated_at >= created, "{row:?}");

    let history = r.history(&id);
    assert_eq!(history.len(), 2, "{history:#?}");
    assert_eq!(history[0], first[0], "version 1's after-image is unchanged");
    let image = &history[1];
    assert_eq!((image.version, image.op.as_str()), (2, "update"));
    assert_eq!(json_of(&image.data), data, "the whole row, not the patch");
    assert_eq!(image.call_seq, Some(2));
    assert!(image.seq > history[0].seq, "after-images are ordered");
    assert_eq!(image.step_run_id.as_deref(), Some(a.id.as_str()));
    let written = r.entries("row.written");
    assert_eq!(written.len(), 2);
    assert_eq!(written[1]["args"]["op"], "update");
    assert_eq!(written[1]["args"]["version"], 2);
}

/// Row 4 (owner Q2): an update that names a version the row is not at is
/// refused in words, `StoreError::RowRefused`, naming the row (a store id)
/// and its current version; nothing changes: the row's data, version and
/// taint, its every after-image, the call (still `issued`, which the engine
/// then finishes `failed`), the audit. Both ways: a version behind (1 while
/// at 2) and one ahead (2 while at 1).
#[test]
fn a_stale_update_is_refused_in_words_and_nothing_changes() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let moved = r.insert(&a, 1, &json!({ "ref": "r-1", "title": "first" }));
    r.update(&a, 2, &moved, 1, &json!({ "title": "second" }))
        .unwrap();
    let still = r.insert(&a, 3, &json!({ "ref": "r-2" }));

    for (row, stale, current, seq) in [(&moved, 1, 2, 4), (&still, 2, 1, 5)] {
        let before = r.state();
        let call = r.issue(&a, seq);
        let issued = r.state();

        let refused = r
            .write_at(
                &a,
                seq,
                &call,
                update_op(
                    Uuid::parse_str(row).unwrap(),
                    stale,
                    &json!({ "title": CANARY_PATCH }),
                ),
            )
            .expect_err("a stale version is refused");

        let words = refused.to_string();
        assert!(
            matches!(&refused, StoreError::RowRefused(_)),
            "RowRefused: {refused:?}"
        );
        assert!(words.contains(row.as_str()), "names the row: {words}");
        assert!(
            words.contains(&format!("version {current}")),
            "names its version {current}: {words}"
        );
        assert!(!words.contains(CANARY_PATCH), "no model value: {words}");
        assert_eq!(r.state(), issued, "nothing changed");
        assert_eq!(r.tool_call(&a, &call).status, "issued");
        assert_eq!(r.tool_call(&a, &call).result_kind, None, "no kind");
        assert_eq!(before.rows, issued.rows);
        assert_eq!(before.history, issued.history);
    }
    assert_eq!(r.row(&moved).version, 2);
    assert_eq!(r.row(&still).version, 1);
}

/// Row 4 (owner Q2): the old version stays readable. After an update,
/// version 1 is read from `row_history` with its data as written; after a
/// refused update, the current row is read with `Reader::table_row_by_id` exactly as it
/// was (byte for byte), and the history holds the same after-images.
#[test]
fn the_old_version_stays_readable_from_history_and_as_the_current_row() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let v1 = json!({ "ref": "r-1", "title": "first", "count": 3 });
    let id = r.insert(&a, 1, &v1);
    let v1_text = r.row(&id).data;
    r.update(&a, 2, &id, 1, &json!({ "title": "second" }))
        .unwrap();

    let history = r.history(&id);
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].version, 1);
    assert_eq!(history[0].data, v1_text, "version 1, byte for byte");
    assert_eq!(json_of(&history[0].data), v1);
    let current = r.row(&id);
    assert_eq!(current.version, 2);

    assert!(
        r.update(&a, 3, &id, 1, &json!({ "title": "third" }))
            .is_err()
    );

    assert_eq!(r.row(&id), current, "the current row, as it was");
    assert_eq!(r.history(&id), history, "every version, as it was");
    assert_eq!(json_of(&r.history(&id)[0].data), v1);
}

/// Row 15 (the verify, AD-8): the key is (step run, call sequence). A
/// second write with a key that already has an after-image writes nothing
/// (no row, version, after-image or audit entry), whatever it carries, and
/// `Reader::row_written_at` gives the first write; this holds also once the
/// attempt has finished (the replay is checked first, for story 3.9).
#[test]
fn a_repeated_call_sequence_writes_once() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let call = r.issue(&a, 1);
    let first = Uuid::now_v7();
    r.write_at(&a, 1, &call, insert_op(first, &json!({ "ref": "r-1" })))
        .unwrap();
    let once = r.state();

    r.write_at(
        &a,
        1,
        &call,
        insert_op(Uuid::now_v7(), &json!({ "ref": "r-2" })),
    )
    .expect("a replay is no error");
    // ToolGate slice, review round 1 (R4): a write of another op kind at
    // the key is no replay; it is refused (`a_replay_is_the_same_write_or_
    // refused`), and nothing changes either way.
    r.write_at(&a, 1, &call, update_op(first, 1, &json!({ "ref": "r-3" })))
        .expect_err("another op kind at a used key is no replay");

    assert_eq!(r.state(), once, "the second write wrote nothing");
    let written = r
        .reader()
        .row_written_at(&a.id, 1)
        .unwrap()
        .expect("the first write");
    assert_eq!(written.row_id, first.to_string());
    assert_eq!((written.version, written.op.as_str()), (1, "insert"));
    assert_eq!(written.tool_call_id.as_deref(), Some(call.as_str()));
    let receipt = r
        .reader()
        .row_receipt(&a.id, 1)
        .unwrap()
        .expect("a receipt");
    assert_eq!(
        (
            receipt.row_id.as_str(),
            receipt.version,
            receipt.op.as_str()
        ),
        (first.to_string().as_str(), 1, "insert"),
        "the receipt is the first write's"
    );
    assert_eq!(r.entries("row.written").len(), 1);

    r.finish(&a);
    let finished = r.state();
    r.write_at(
        &a,
        1,
        &call,
        insert_op(Uuid::now_v7(), &json!({ "ref": "r-4" })),
    )
    .expect("a replay of a finished attempt is no error");
    assert_eq!(r.state(), finished);
    assert_eq!(r.reader().row_written_at(&a.id, 2).unwrap(), None);
}

/// Row 16: a loop-back's attempt 2 of the same step starts its call
/// sequences at 1 again; its write is a new write, keyed by its own step
/// run, and both after-images say which attempt wrote them.
#[test]
fn the_key_is_the_attempt_not_the_step() {
    let r = rig_with(&["collect", "check"]);
    let first = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let one = r.insert(&first, 1, &json!({ "ref": "r-1" }));
    r.finish(&first);
    r.loop_back(&r.run, "check", "collect");
    let second = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    assert_eq!(r.step(&second.id).attempt, 2);

    let two = r.insert(&second, 1, &json!({ "ref": "r-2" }));

    assert_ne!(one, two);
    assert_eq!(r.reader().table_rows_of(TABLE).unwrap().len(), 2);
    let reader = r.reader();
    let w1 = reader.row_written_at(&first.id, 1).unwrap().unwrap();
    let w2 = reader.row_written_at(&second.id, 1).unwrap().unwrap();
    assert_eq!((w1.row_id.as_str(), w1.call_seq), (one.as_str(), Some(1)));
    assert_eq!((w2.row_id.as_str(), w2.call_seq), (two.as_str(), Some(1)));
    assert_eq!(w1.step_run_id.as_deref(), Some(first.id.as_str()));
    assert_eq!(w2.step_run_id.as_deref(), Some(second.id.as_str()));
    assert_eq!(r.entries("row.written").len(), 2);
}

/// Row 17 (and row 5): the writer checks a write against the journal and
/// refuses, in words and with nothing written (no row, no after-image, the
/// call as it was, no audit entry): an actor that is not a Seat; a Seat of
/// another run, step or seat; no `issued` `table_write` call at the key
/// (none, another tool's, a refused one, another call's id); a table that is
/// not the attempt's (or an attempt that declares none); an insert over an
/// existing row id; a version that is not the current one; a row the table
/// does not hold (words without its id); an attempt that is not running.
#[test]
fn the_writer_refuses_a_write_that_is_not_its_attempts() {
    let r = rig_with(&["collect", "check", "plain"]);
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let held = r.insert(&a, 1, &json!({ "ref": "r-1" }));
    let call = r.issue(&a, 2);
    let other_tool = r.record(&a, 3, "fixture_read", "issued");
    let refused_call = r.record(&a, 4, "table_write", "refused");
    let fresh = || insert_op(Uuid::now_v7(), &json!({ "ref": "r-2" }));
    let other = r.new_run(&["collect"]);
    let seat_as = |seat: &str, run: Uuid, step: &str| Actor::Seat {
        seat: seat.into(),
        run,
        step: step.into(),
    };
    let unknown = Uuid::now_v7();

    let cases: Vec<(&str, Actor, RowWrite)> = vec![
        (
            "the engine, not a seat",
            engine(),
            row_write(&a, 2, &call, TABLE, fresh(), Taint::Clean),
        ),
        (
            "a person, not a seat",
            you_reviewer(),
            row_write(&a, 2, &call, TABLE, fresh(), Taint::Clean),
        ),
        (
            "a seat of another run",
            seat_as(SEAT, other.uuid, "collect"),
            row_write(&a, 2, &call, TABLE, fresh(), Taint::Clean),
        ),
        (
            "a seat in another step",
            seat_as(SEAT, r.run.uuid, "check"),
            row_write(&a, 2, &call, TABLE, fresh(), Taint::Clean),
        ),
        (
            "another seat",
            seat_as("seat-other", r.run.uuid, "collect"),
            row_write(&a, 2, &call, TABLE, fresh(), Taint::Clean),
        ),
        (
            "no tool call at its call sequence",
            a.actor(),
            row_write(&a, 9, &new_id(), TABLE, fresh(), Taint::Clean),
        ),
        (
            "another tool's call",
            a.actor(),
            row_write(&a, 3, &other_tool, TABLE, fresh(), Taint::Clean),
        ),
        (
            "a refused call",
            a.actor(),
            row_write(&a, 4, &refused_call, TABLE, fresh(), Taint::Clean),
        ),
        (
            "another call's id at the key",
            a.actor(),
            row_write(&a, 2, &other_tool, TABLE, fresh(), Taint::Clean),
        ),
        (
            "a table that is not the attempt's",
            a.actor(),
            row_write(&a, 2, &call, OTHER_TABLE, fresh(), Taint::Clean),
        ),
        (
            "an insert over an existing row id",
            a.actor(),
            row_write(
                &a,
                2,
                &call,
                TABLE,
                insert_op(Uuid::parse_str(&held).unwrap(), &json!({ "ref": "r-9" })),
                Taint::Clean,
            ),
        ),
        (
            "a version before the current one",
            a.actor(),
            row_write(
                &a,
                2,
                &call,
                TABLE,
                update_op(Uuid::parse_str(&held).unwrap(), 0, &json!({ "title": "x" })),
                Taint::Clean,
            ),
        ),
        (
            "a version past the current one",
            a.actor(),
            row_write(
                &a,
                2,
                &call,
                TABLE,
                update_op(Uuid::parse_str(&held).unwrap(), 2, &json!({ "title": "x" })),
                Taint::Clean,
            ),
        ),
        (
            "a row the table does not hold",
            a.actor(),
            row_write(
                &a,
                2,
                &call,
                TABLE,
                update_op(unknown, 1, &json!({ "title": "x" })),
                Taint::Clean,
            ),
        ),
    ];
    for (what, actor, write) in cases {
        let before = r.state();
        let refused = r
            .apply(actor, write)
            .expect_err(&format!("{what}: refused"));
        let words = refused.to_string();
        assert!(!words.trim().is_empty(), "{what}: in words");
        assert_eq!(r.state(), before, "{what}: nothing written");
        assert_eq!(r.tool_call(&a, &call).status, "issued", "{what}");
        if what == "a row the table does not hold" {
            assert!(
                matches!(&refused, StoreError::RowRefused(_)),
                "{what}: {refused:?}"
            );
            assert!(words.contains("does not hold"), "{what}: {words}");
            assert!(
                !words.contains(&unknown.to_string()),
                "{what}: the id is not echoed: {words}"
            );
        }
    }

    // An attempt that declares no table writes none.
    r.finish(&a);
    let b = r.begin(&r.run, "check", None, None, Taint::Clean);
    let b_call = r.issue(&b, 1);
    let before = r.state();
    assert!(
        r.write_at(&b, 1, &b_call, fresh()).is_err(),
        "an attempt with no table"
    );
    assert_eq!(r.state(), before);

    // An attempt that is no longer running writes nothing.
    let c = r.begin(&r.run, "plain", Some(TABLE), None, Taint::Clean);
    let c_call = r.issue(&c, 1);
    r.finish(&c);
    let before = r.state();
    assert!(
        r.write_at(&c, 1, &c_call, fresh()).is_err(),
        "a finished attempt"
    );
    assert_eq!(r.state(), before);
    assert_eq!(
        r.reader().table_rows_of(TABLE).unwrap().len(),
        1,
        "only the first row"
    );
}

/// Row 18: `row_history` is append-only and `table_rows` changes only as
/// the writer changes it. A raw `UPDATE` or `DELETE` of an after-image, an
/// `INSERT OR REPLACE` over one (by its `seq`, or a copy meeting its version
/// key), a `DELETE` or a replace of a row, and an `UPDATE` that does not
/// raise the version by one, are aborted; every row stays as it was. (That
/// a row's table, id and creation time never change is
/// `a_rows_table_id_and_creation_time_never_change`, where that guard alone
/// can abort: review triage round 1, R1.)
#[test]
fn rows_and_history_are_append_only() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let id = r.insert(&a, 1, &json!({ "ref": "r-1", "title": "first" }));
    r.update(&a, 2, &id, 1, &json!({ "title": "second" }))
        .unwrap();
    let before = r.state();
    let c = r.raw();
    c.execute_batch(
        "CREATE TEMP TABLE image AS SELECT * FROM row_history WHERE seq = (SELECT MIN(seq) FROM row_history);
         UPDATE image SET seq = (SELECT MAX(seq) + 100 FROM row_history);
         CREATE TEMP TABLE image_same_key AS SELECT * FROM image;
         UPDATE image_same_key SET seq = seq + 1, version = 99;",
    )
    .unwrap();

    for sql in [
        "UPDATE row_history SET data = '{\"ref\":\"forged\"}'",
        "UPDATE row_history SET taint = 1",
        "UPDATE row_history SET version = version + 10",
        "DELETE FROM row_history",
        "INSERT OR REPLACE INTO row_history SELECT * FROM row_history WHERE seq = (SELECT MIN(seq) FROM row_history)",
        "INSERT OR REPLACE INTO row_history SELECT * FROM temp.image",
        "INSERT INTO row_history SELECT * FROM temp.image",
        "INSERT OR REPLACE INTO row_history SELECT * FROM temp.image_same_key",
        "INSERT INTO row_history SELECT * FROM temp.image_same_key",
        "DELETE FROM table_rows",
        "INSERT OR REPLACE INTO table_rows SELECT * FROM table_rows",
        "UPDATE table_rows SET version = version + 2",
        "UPDATE table_rows SET version = version - 1",
        "UPDATE table_rows SET data = '{\"ref\":\"forged\"}'",
    ] {
        assert!(c.execute_batch(sql).is_err(), "aborted: {sql}");
    }
    drop(c);
    assert_eq!(r.state(), before, "every row as it was");
}

/// Row 18: no row changes without its after-image, written first in the
/// same transaction: a raw insert of a row with no after-image, and a raw
/// update to the next version with none, are aborted.
#[test]
fn a_row_change_without_its_after_image_is_aborted() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let id = r.insert(&a, 1, &json!({ "ref": "r-1" }));
    let before = r.state();
    let c = r.raw();
    let at = "2026-11-24T10:00:00.000Z";

    let insert = c.execute(
        "INSERT INTO table_rows (table_id, row_id, data, version, taint, created_at, updated_at) VALUES (?1, ?2, '{\"ref\":\"r-9\"}', 1, 0, ?3, ?3)",
        params![TABLE, Uuid::now_v7().to_string(), at],
    );
    assert!(insert.is_err(), "a row with no after-image: {insert:?}");
    let update = c.execute(
        "UPDATE table_rows SET version = version + 1, data = '{\"ref\":\"r-1\",\"title\":\"unrecorded\"}', updated_at = ?2 WHERE row_id = ?1",
        params![id, at],
    );
    assert!(
        update.is_err(),
        "a new version with no after-image: {update:?}"
    );
    drop(c);
    assert_eq!(r.state(), before);
}

/// Row 19: the after-image, the row and the call's finish commit together.
/// A failure forced after the after-image is written (a trigger made by the
/// test aborts the row's insert, then the call's finish) leaves no row, no
/// after-image, the call `issued` and no audit entry; once the cause is
/// gone the same key writes once.
#[test]
fn a_refused_write_leaves_nothing() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let call = r.issue(&a, 1);
    let row = Uuid::now_v7();
    let data = json!({ "ref": "r-1" });

    for (what, trigger) in [
        (
            "the row's insert",
            "CREATE TRIGGER forced_failure BEFORE INSERT ON table_rows BEGIN SELECT RAISE(ABORT, 'forced by the test'); END;",
        ),
        (
            "the call's finish",
            "CREATE TRIGGER forced_failure BEFORE UPDATE ON tool_calls BEGIN SELECT RAISE(ABORT, 'forced by the test'); END;",
        ),
    ] {
        let before = r.state();
        r.raw().execute_batch(trigger).unwrap();

        let failed = r.write_at(&a, 1, &call, insert_op(row, &data));

        r.raw()
            .execute_batch("DROP TRIGGER forced_failure;")
            .unwrap();
        assert!(failed.is_err(), "{what}: the write fails");
        assert_eq!(r.state(), before, "{what}: nothing is left");
        assert_eq!(r.count("row_history"), 0, "{what}: no after-image");
        assert_eq!(r.tool_call(&a, &call).status, "issued", "{what}");
        assert_eq!(r.tool_call(&a, &call).result_kind, None, "{what}: no kind");
        assert_eq!(r.reader().row_written_at(&a.id, 1).unwrap(), None, "{what}");
    }

    r.write_at(&a, 1, &call, insert_op(row, &data)).unwrap();
    assert_eq!(r.reader().table_rows_of(TABLE).unwrap().len(), 1);
    assert_eq!(r.count("row_history"), 1);
    assert_eq!(r.tool_call(&a, &call).status, "succeeded");
}

/// Row 13 (AD-10 restated 2026-11-24): a row's taint only rises. A tainted
/// attempt's write is tainted at rest (the writer takes the attempt's
/// committed taint, whatever the command says); a clean attempt's later
/// version keeps the row tainted, and its after-image records the row's
/// taint after the write (1), not the writer's. (That the taint guard
/// itself aborts a raw update lowering it is
/// `a_rows_taint_never_falls_by_its_own_guard`: review triage round 1, R1.)
#[test]
fn row_taint_only_rises() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Tainted);
    let id = r.insert(&a, 1, &json!({ "ref": "r-1" }));
    r.update(&a, 2, &id, 1, &json!({ "title": "second" }))
        .unwrap();
    r.finish(&a);
    assert_eq!(r.row(&id).taint, Taint::Tainted);
    assert!(
        r.history(&id).iter().all(|h| h.taint == Taint::Tainted),
        "{:#?}",
        r.history(&id)
    );
    assert!(
        r.entries("row.written")
            .iter()
            .all(|e| audited_tainted(&e["args"]["taint"])),
        "the audit carries the taint"
    );

    let b = r.begin(&r.run, "check", Some(TABLE), None, Taint::Clean);
    r.update(&b, 1, &id, 2, &json!({ "title": "third" }))
        .unwrap();

    let row = r.row(&id);
    assert_eq!((row.version, row.taint), (3, Taint::Tainted));
    let v3 = r.history(&id).pop().unwrap();
    assert_eq!(v3.version, 3);
    assert_eq!(
        v3.taint,
        Taint::Tainted,
        "the row's taint after the write, not the clean writer's"
    );
    assert_eq!(v3.step_run_id.as_deref(), Some(b.id.as_str()));
}

/// Row 22 (AD-25): `row.written` holds ids, the op, the version, the call
/// sequence, the step run, a column count and the taint, never row data:
/// no canary of the data or the patch, and no hash of either, anywhere in
/// the export, and no `*_sha256` argument on a row entry; the chain
/// verifies.
#[test]
fn row_audit_holds_ids_never_data() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let data = json!({ "ref": "r-1", "title": CANARY_DATA });
    let patch = json!({ "title": CANARY_PATCH });
    let id = r.insert(&a, 1, &data);
    r.update(&a, 2, &id, 1, &patch).unwrap();

    let written = r.entries("row.written");
    assert_eq!(written.len(), 2, "{written:#?}");
    for (e, (op, version)) in written.iter().zip([("insert", 1), ("update", 2)]) {
        assert_eq!(e["target"], format!("row:{TABLE}/{id}"));
        assert_eq!(e["args"]["op"], op);
        assert_eq!(e["args"]["version"], version);
        assert_eq!(e["args"]["call_seq"], version);
        assert_eq!(e["args"]["step_run_id"], a.id.as_str());
        assert!(audited_clean(&e["args"]["taint"]), "{e}");
        let args = e["args"].as_object().expect("args");
        assert!(
            args.keys().all(|k| !k.ends_with("_sha256")),
            "no hash argument: {e}"
        );
    }
    let export = serde_json::to_string(&r.audit()).unwrap();
    for secret in [
        CANARY_DATA.to_string(),
        CANARY_PATCH.to_string(),
        sha256_hex(CANARY_DATA),
        sha256_hex(CANARY_PATCH),
        sha256_hex(&data.to_string()),
        sha256_hex(&patch.to_string()),
        sha256_hex(&r.row(&id).data),
    ] {
        assert!(!export.contains(&secret), "{secret} in {export}");
    }
    assert_eq!(r.reader().verify_audit_chain().unwrap(), None);
}

// ------------------------------------------------- the store slice's own rows

/// Row 12 (owner: each row's own taint, stored on write): a row's taint is
/// the highest of the attempt's committed taint, the write's taint and the
/// row's. An attempt tainted at its start writes with a clean command: the
/// row, its after-image and its `row.written` entry carry taint 1. A clean
/// attempt whose taint a later call raised (a tool result it carried) does
/// too: the writer reads the committed taint, not the start's. A clean
/// attempt's write that says tainted is tainted. A clean write of a clean
/// attempt is clean.
#[test]
fn a_tainted_attempts_row_stores_its_taint() {
    let r = rig_with(&["collect", "check", "plain", "more"]);

    let tainted = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Tainted);
    let one = r.insert(&tainted, 1, &json!({ "ref": "r-1" }));
    assert_eq!(r.row(&one).taint, Taint::Tainted);
    assert_eq!(r.history(&one)[0].taint, Taint::Tainted);
    r.finish(&tainted);

    let raised = r.begin(&r.run, "check", Some(TABLE), None, Taint::Clean);
    r.exec(WriteCommand::ReserveCall {
        step_run_id: raised.id.clone(),
        call_id: new_id(),
        call_seq: 1,
        reserved_micro_usd: 0,
        rate_snapshot: "none".into(),
        taint: Taint::Tainted,
        purpose: CallPurpose::Turn,
        prompt: vec![],
    })
    .unwrap();
    assert_eq!(r.step(&raised.id).taint, Taint::Tainted, "committed");
    let two = r.insert(&raised, 1, &json!({ "ref": "r-2" }));
    assert_eq!(r.row(&two).taint, Taint::Tainted);
    assert_eq!(r.history(&two)[0].taint, Taint::Tainted);

    let clean = r.begin(&r.run, "plain", Some(TABLE), None, Taint::Clean);
    let call = r.issue(&clean, 1);
    let three = Uuid::now_v7();
    r.apply(
        clean.actor(),
        row_write(
            &clean,
            1,
            &call,
            TABLE,
            insert_op(three, &json!({ "ref": "r-3" })),
            Taint::Tainted,
        ),
    )
    .unwrap();
    assert_eq!(
        r.row(&three.to_string()).taint,
        Taint::Tainted,
        "the write's taint"
    );
    let four = r.insert(&clean, 2, &json!({ "ref": "r-4" }));
    assert_eq!(
        r.row(&four).taint,
        Taint::Clean,
        "a clean write stays clean"
    );
    assert_eq!(r.history(&four)[0].taint, Taint::Clean);

    let written = r.entries("row.written");
    assert_eq!(written.len(), 4, "{written:#?}");
    for (n, e) in written.iter().enumerate() {
        if n < 3 {
            assert!(audited_tainted(&e["args"]["taint"]), "{e}");
        } else {
            assert!(audited_clean(&e["args"]["taint"]), "{e}");
        }
    }
}

/// Row 44 (AD-8; AD-10 as amended, with Winston's wording "on its tool
/// call's journal row"): a succeeded write marks its call's journal row
/// `engine` / `tool:table_write`, an insert and an update alike; the
/// data-free receipt at the key names the table, the row, the version, the
/// op, the taint, the call and its tool call id, and the insert's column
/// count.
#[test]
fn a_succeeded_write_journals_an_engine_result_kind() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let id = r.insert(
        &a,
        1,
        &json!({ "ref": "r-1", "title": "first", "amount": 2 }),
    );
    r.update(&a, 2, &id, 1, &json!({ "title": "second" }))
        .unwrap();

    let calls = r.reader().tool_calls(&a.id).unwrap();
    assert_eq!(calls.len(), 2, "{calls:#?}");
    for call in &calls {
        assert_eq!(call.status, "succeeded");
        assert_eq!(call.result_kind.as_deref(), Some("engine"), "{call:?}");
        assert_eq!(
            call.result_label.as_deref(),
            Some("tool:table_write"),
            "{call:?}"
        );
    }
    let reader = r.reader();
    for (seq, version, op) in [(1, 1, "insert"), (2, 2, "update")] {
        let receipt = reader
            .row_receipt(&a.id, seq)
            .unwrap()
            .unwrap_or_else(|| panic!("a receipt at call {seq}"));
        assert_eq!(receipt.table_id, TABLE);
        assert_eq!(receipt.row_id, id);
        assert_eq!((receipt.version, receipt.op.as_str()), (version, op));
        assert_eq!(receipt.taint, Taint::Clean);
        assert_eq!(receipt.step_run_id, a.id);
        assert_eq!(receipt.call_seq, seq);
        assert_eq!(receipt.tool_call_id, calls[seq as usize - 1].id);
    }
    assert_eq!(reader.row_receipt(&a.id, 1).unwrap().unwrap().columns, 3);
    assert_eq!(
        reader.row_receipt(&a.id, 3).unwrap(),
        None,
        "no write, no receipt"
    );
}

/// Row 44: `FinishToolCall` journals the result's kind `untrusted` and the
/// label `tool:<tool>`, for a `succeeded` and a `failed` call alike (a
/// runner's result, or an error the model reads); a `refused` call and a
/// call still `issued` have no kind.
#[test]
fn finish_tool_call_journals_untrusted() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let read = r.record(&a, 1, "fixture_read", "issued");
    let failed = r.issue(&a, 2);
    let refused = r.record(&a, 3, "table_write", "refused");
    let open = r.issue(&a, 4);
    for (call, status) in [(&read, "succeeded"), (&failed, "failed")] {
        r.exec(WriteCommand::FinishToolCall {
            tool_call_id: call.clone(),
            status: status.into(),
        })
        .unwrap();
    }

    for (call, status, kind, label) in [
        (
            &read,
            "succeeded",
            Some("untrusted"),
            Some("tool:fixture_read"),
        ),
        (
            &failed,
            "failed",
            Some("untrusted"),
            Some("tool:table_write"),
        ),
        (&refused, "refused", None, None),
        (&open, "issued", None, None),
    ] {
        let row = r.tool_call(&a, call);
        assert_eq!(row.status, status, "{row:?}");
        assert_eq!(
            (row.result_kind.as_deref(), row.result_label.as_deref()),
            (kind, label),
            "{row:?}"
        );
    }
}

/// Row 44: the receipt a result is rebuilt from carries no row data: with
/// canaries in an insert's data and an update's patch, neither canary nor
/// its hash nor the hash of the row's data is anywhere in either receipt.
#[test]
fn the_receipt_carries_no_row_data() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let data = json!({ "ref": "r-1", "title": CANARY_DATA, "extra": { "note": CANARY_DATA } });
    let patch = json!({ "title": CANARY_PATCH });
    let id = r.insert(&a, 1, &data);
    r.update(&a, 2, &id, 1, &patch).unwrap();
    let row_data = r.row(&id).data;

    let reader = r.reader();
    for seq in [1, 2] {
        let receipt = reader.row_receipt(&a.id, seq).unwrap().expect("a receipt");
        let seen = format!("{receipt:?}");
        for secret in [
            CANARY_DATA.to_string(),
            CANARY_PATCH.to_string(),
            sha256_hex(CANARY_DATA),
            sha256_hex(CANARY_PATCH),
            sha256_hex(&row_data),
            row_data.clone(),
        ] {
            assert!(!seen.contains(&secret), "call {seq}: {seen}");
        }
        assert!(seen.contains(&id), "it names the row: {seen}");
    }
}

// --------------------------------------------- review triage round 1 (R1–R12)
//
// The manager's triage of the two reviews of the store slice (2026-11-24,
// the store plan's "Review triage round 1"). Each test reaches the guard it
// names: the state it sets up leaves that guard alone to abort, and the
// test asserts that guard's own words. Written before the fixes.

/// A person's actor, as `row_history.actor` holds one (no seat provenance).
const HUMAN: &str = r#"{"kind":"human","person":"owner","hat":"Reviewer"}"#;

/// A raw after-image of `row` in `entries` at `version`, by `actor`, with no
/// seat provenance (a person's write, made by hand).
fn raw_image(
    c: &Connection,
    row: &str,
    version: i64,
    data: &str,
    taint: i64,
    actor: &str,
) -> rusqlite::Result<usize> {
    let op = if version == 1 { "insert" } else { "update" };
    c.execute(
        "INSERT INTO row_history (table_id, row_id, version, data, taint, actor, op, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '2026-11-24T10:00:00.000Z')",
        params![TABLE, row, version, data, taint, actor, op],
    )
}

/// The words a raw statement was aborted with (it must fail).
fn aborted_with(result: rusqlite::Result<usize>, what: &str) -> String {
    match result {
        Ok(n) => panic!("{what}: must abort, changed {n} row(s)"),
        Err(e) => e.to_string(),
    }
}

/// Drops every trigger on `table` whose SQL does not hold `keep`, in this
/// test's own database, so the one guard left is the only thing that can
/// abort a statement (for a guard that others always reach first).
fn leave_only(c: &Connection, table: &str, keep: &str) {
    let names: Vec<String> = c
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = ?1 AND sql NOT LIKE '%' || ?2 || '%'",
        )
        .unwrap()
        .query_map(params![table, keep], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(!names.is_empty(), "other triggers on {table} to set aside");
    for name in names {
        c.execute_batch(&format!("DROP TRIGGER \"{name}\";"))
            .unwrap();
    }
}

/// R1 (row 18): a row's table, id and creation time never change, and it
/// is that guard which says so. With a person's after-image at the next
/// version (same data, same taint) in place, an update to that version that
/// names `table_id`, `row_id` or `created_at` passes every other guard and
/// is aborted with "a row's table, id and creation time never change"; the
/// same update naming none of them goes through (the control).
#[test]
fn a_rows_table_id_and_creation_time_never_change() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let id = r.insert(&a, 1, &json!({ "ref": "r-1" }));
    let data = r.row(&id).data;
    let c = r.raw();
    raw_image(&c, &id, 2, &data, 0, HUMAN).expect("a person's after-image at version 2");
    let before = r.state();

    for (what, sql) in [
        (
            "table_id",
            "UPDATE table_rows SET table_id = table_id, version = version + 1 WHERE row_id = ?1",
        ),
        (
            "row_id",
            "UPDATE table_rows SET row_id = row_id, version = version + 1 WHERE row_id = ?1",
        ),
        (
            "created_at",
            "UPDATE table_rows SET created_at = '2020-01-01T00:00:00.000Z', version = version + 1 WHERE row_id = ?1",
        ),
    ] {
        let words = aborted_with(c.execute(sql, params![id]), what);
        assert!(
            words.contains("table, id and creation time never change"),
            "{what}: the fixed-fields guard: {words}"
        );
    }
    assert_eq!(r.state(), before, "every row as it was");

    let control = c.execute(
        "UPDATE table_rows SET version = version + 1, updated_at = '2026-11-24T10:00:01.000Z' WHERE row_id = ?1",
        params![id],
    );
    assert_eq!(
        control.ok(),
        Some(1),
        "the same update, naming none of them"
    );
}

/// R1 (row 13): a row's taint only rises, and it is that guard which says
/// so. A person's after-image that would lower a tainted row's taint is
/// itself refused (R5's guard, so no such image exists); with the row's
/// other triggers set aside in this test's database, a raw update lowering
/// the taint is aborted with "table_rows.taint only rises".
#[test]
fn a_rows_taint_never_falls_by_its_own_guard() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Tainted);
    let id = r.insert(&a, 1, &json!({ "ref": "r-1" }));
    let data = r.row(&id).data;
    let c = r.raw();

    let words = aborted_with(
        raw_image(&c, &id, 2, &data, 0, HUMAN),
        "a lower-taint after-image",
    );
    assert!(
        words.contains("never lowers its row's taint"),
        "R5's guard: {words}"
    );

    leave_only(&c, "table_rows", "only rises");
    let words = aborted_with(
        c.execute(
            "UPDATE table_rows SET taint = 0 WHERE row_id = ?1",
            params![id],
        ),
        "a lowered taint",
    );
    assert!(words.contains("table_rows.taint only rises"), "{words}");
    drop(c);
    assert_eq!(r.row(&id).taint, Taint::Tainted);
}

/// R1 (row 44): each guard of the result kind is the one that aborts. On a
/// call already finished `succeeded` (and so past the "set when it
/// finishes" guard), marking `engine` on a `table_write` call with no
/// after-image at its key, or on another tool's call, is aborted with "an
/// engine result answers only a table_write call whose row was written";
/// changing a set kind or label (engine to untrusted or NULL, a label
/// renamed) is aborted with "a tool call's result kind is set once"; a kind
/// that is neither fails the column's CHECK. (Untrusted to engine on
/// another tool's call meets both guards, so either's words will do.)
#[test]
fn each_result_kind_guard_aborts_with_its_own_words() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    r.insert(&a, 1, &json!({ "ref": "r-1" }));
    let written = r.reader().tool_calls(&a.id).unwrap()[0].id.clone();
    let bare = r.issue(&a, 2);
    let other = r.record(&a, 3, "fixture_read", "issued");
    let finished = r.record(&a, 4, "fixture_read", "issued");
    r.exec(WriteCommand::FinishToolCall {
        tool_call_id: finished.clone(),
        status: "succeeded".into(),
    })
    .unwrap();
    let c = r.raw();
    for id in [&bare, &other] {
        c.execute(
            "UPDATE tool_calls SET status = 'succeeded', finished_at = '2026-11-24T10:00:00.000Z' WHERE id = ?1",
            params![id],
        )
        .expect("a call with no kind finishes by hand");
    }
    let before = r.state();

    for (what, sql, id, guards) in [
        (
            "engine on a table_write call with no after-image",
            "UPDATE tool_calls SET result_kind = 'engine', result_label = 'tool:table_write' WHERE id = ?1",
            bare.as_str(),
            &["an engine result answers only a table_write call whose row was written"][..],
        ),
        (
            "engine on another tool's call",
            "UPDATE tool_calls SET result_kind = 'engine', result_label = 'tool:table_write' WHERE id = ?1",
            other.as_str(),
            &["an engine result answers only a table_write call whose row was written"][..],
        ),
        (
            "engine changed to untrusted",
            "UPDATE tool_calls SET result_kind = 'untrusted', result_label = 'tool:table_write' WHERE id = ?1",
            written.as_str(),
            &["result kind is set once"][..],
        ),
        (
            "engine cleared",
            "UPDATE tool_calls SET result_kind = NULL, result_label = NULL WHERE id = ?1",
            written.as_str(),
            &["result kind is set once"][..],
        ),
        (
            "a set label renamed",
            "UPDATE tool_calls SET result_label = 'tool:renamed' WHERE id = ?1",
            finished.as_str(),
            &["result kind is set once"][..],
        ),
        (
            "untrusted changed to engine",
            "UPDATE tool_calls SET result_kind = 'engine', result_label = 'tool:table_write' WHERE id = ?1",
            finished.as_str(),
            &[
                "result kind is set once",
                "an engine result answers only a table_write call whose row was written",
            ][..],
        ),
        (
            "a kind that is neither",
            "UPDATE tool_calls SET result_kind = 'trusted', result_label = 'tool:fixture_read' WHERE id = ?1",
            other.as_str(),
            &["CHECK constraint failed"][..],
        ),
    ] {
        let words = aborted_with(c.execute(sql, params![id]), what);
        assert!(
            guards.iter().any(|g| words.contains(g)),
            "{what}: {guards:?}: {words}"
        );
    }
    drop(c);
    assert_eq!(r.state(), before, "every row as it was");
}

/// R2: once a call's result kind is set, its journal row never changes: a
/// raw change of its `tool`, `status`, `step_run_id`, `call_seq` or `id`, a
/// move of its rowid and an `UPDATE OR REPLACE` of its `call_seq` onto
/// another call's are each aborted with "a tool call with a result kind
/// never changes", for an `engine` and an `untrusted` call alike.
#[test]
fn a_tool_call_with_a_result_kind_never_changes() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    r.insert(&a, 1, &json!({ "ref": "r-1" }));
    let written = r.reader().tool_calls(&a.id).unwrap()[0].id.clone();
    let finished = r.record(&a, 2, "fixture_read", "issued");
    r.exec(WriteCommand::FinishToolCall {
        tool_call_id: finished.clone(),
        status: "failed".into(),
    })
    .unwrap();
    let open = r.issue(&a, 3);
    let elsewhere = r.latest(&r.run, "check").id;
    let before = r.state();
    let c = r.raw();

    for id in [&written, &finished] {
        for (what, sql) in [
            (
                "tool",
                "UPDATE tool_calls SET tool = 'other_tool' WHERE id = ?1",
            ),
            (
                "status",
                "UPDATE tool_calls SET status = 'issued' WHERE id = ?1",
            ),
            (
                "step_run_id",
                "UPDATE tool_calls SET step_run_id = ?2 WHERE id = ?1",
            ),
            (
                "call_seq",
                "UPDATE tool_calls SET call_seq = 99 WHERE id = ?1",
            ),
            (
                "id",
                "UPDATE tool_calls SET id = 'a-forged-call-id' WHERE id = ?1",
            ),
            (
                "rowid",
                "UPDATE tool_calls SET rowid = rowid + 1000 WHERE id = ?1",
            ),
            (
                "UPDATE OR REPLACE onto another call's sequence",
                "UPDATE OR REPLACE tool_calls SET call_seq = 3 WHERE id = ?1",
            ),
        ] {
            let result = if sql.contains("?2") {
                c.execute(sql, params![id, elsewhere])
            } else {
                c.execute(sql, params![id])
            };
            let words = aborted_with(result, what);
            assert!(
                words.contains("a tool call with a result kind never changes"),
                "{what} of {id}: {words}"
            );
        }
    }
    drop(c);
    assert_eq!(r.state(), before, "every call as it was");
    assert_eq!(r.tool_call(&a, &open).status, "issued");
}

/// R3: the rowid-collision attack aborts. With a person's after-image at
/// another row's next version in place (so every other guard passes),
/// `UPDATE OR REPLACE table_rows SET rowid = <a tainted row's rowid>,
/// version = version + 1` on that other row would delete the tainted row (a
/// REPLACE's delete fires no delete trigger while `recursive_triggers` is
/// off, as on this raw connection); it is aborted and both rows stay as
/// they were, the tainted one at its version and taint.
#[test]
fn the_rowid_collision_attack_aborts() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Tainted);
    let tainted = r.insert(&a, 1, &json!({ "ref": "r-1" }));
    r.update(&a, 2, &tainted, 1, &json!({ "title": "second" }))
        .unwrap();
    let other = r.insert(&a, 3, &json!({ "ref": "r-2" }));
    let c = r.raw();
    raw_image(&c, &other, 2, &r.row(&other).data, 1, HUMAN)
        .expect("a person's after-image at the other row's version 2");
    let before = r.state();

    let result = c.execute(
        "UPDATE OR REPLACE table_rows SET rowid = (SELECT rowid FROM table_rows WHERE row_id = ?1), version = version + 1 WHERE row_id = ?2",
        params![tainted, other],
    );

    aborted_with(result, "the rowid collision");
    drop(c);
    assert_eq!(r.state(), before, "both rows as they were");
    let row = r.row(&tainted);
    assert_eq!((row.version, row.taint), (2, Taint::Tainted));
}

/// R3: a row is never replaced by its rowid: with a person's first
/// after-image for a new row id in place (so the after-image guard passes),
/// an `INSERT OR REPLACE` of that row at an existing row's rowid is aborted
/// with "a row is never replaced".
#[test]
fn a_replace_by_rowid_aborts() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Tainted);
    let id = r.insert(&a, 1, &json!({ "ref": "r-1" }));
    let c = r.raw();
    raw_image(&c, "a-new-row-id", 1, &r.row(&id).data, 0, HUMAN)
        .expect("a person's first after-image for the new row id");
    let before = r.state();

    let result = c.execute(
        "INSERT OR REPLACE INTO table_rows (rowid, table_id, row_id, data, version, taint, created_at, updated_at)
         SELECT rowid, table_id, 'a-new-row-id', data, 1, 0, created_at, updated_at FROM table_rows WHERE row_id = ?1",
        params![id],
    );

    let words = aborted_with(result, "a replace by rowid");
    assert!(words.contains("a row is never replaced"), "{words}");
    drop(c);
    assert_eq!(r.state(), before);
}

/// R3: a row is never re-created. Should a row ever be gone (here its
/// delete guard is set aside in this test's database and the row deleted
/// by hand, as the rowid attack would), inserting it again at version 1
/// with its clean first after-image is aborted with "a row is never
/// re-created", because its history goes beyond version 1.
#[test]
fn a_row_is_never_re_created() {
    let r = rig();
    let clean = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let id = r.insert(&clean, 1, &json!({ "ref": "r-1" }));
    r.finish(&clean);
    let tainted = r.begin(&r.run, "check", Some(TABLE), None, Taint::Tainted);
    r.update(&tainted, 1, &id, 1, &json!({ "title": "second" }))
        .unwrap();
    let first = r.history(&id)[0].clone();
    assert_eq!(first.taint, Taint::Clean, "version 1 was clean");
    let c = r.raw();
    let deletes: Vec<String> = c
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'table_rows' AND sql LIKE '%BEFORE DELETE%'",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for name in deletes {
        c.execute_batch(&format!("DROP TRIGGER \"{name}\";"))
            .unwrap();
    }
    c.execute("DELETE FROM table_rows WHERE row_id = ?1", params![id])
        .unwrap();

    let words = aborted_with(
        c.execute(
            "INSERT INTO table_rows (table_id, row_id, data, version, taint, created_at, updated_at) VALUES (?1, ?2, ?3, 1, 0, ?4, ?4)",
            params![TABLE, id, first.data, first.created_at],
        ),
        "a re-created row",
    );
    assert!(words.contains("a row is never re-created"), "{words}");
}

/// R3: every store connection turns `recursive_triggers` on (so a
/// REPLACE's delete fires the delete guards), beside `foreign_keys`.
/// `testing::configured_pragma` opens a database the way the store opens
/// its connections and reads the pragma.
#[test]
fn store_connections_turn_recursive_triggers_on() {
    let r = rig();
    assert_eq!(
        am_store::testing::configured_pragma(&r.db, "foreign_keys").unwrap(),
        1,
        "the control: foreign keys are on"
    );
    assert_eq!(
        am_store::testing::configured_pragma(&r.db, "recursive_triggers").unwrap(),
        1
    );
}

/// R5: an after-image never lowers its row's taint: a person's after-image
/// at a tainted row's next version with taint 0 is aborted with "an
/// after-image never lowers its row's taint"; with taint 1 it is accepted
/// (the control).
#[test]
fn an_after_image_never_lowers_its_rows_taint() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Tainted);
    let id = r.insert(&a, 1, &json!({ "ref": "r-1" }));
    let data = r.row(&id).data;
    let c = r.raw();

    let words = aborted_with(raw_image(&c, &id, 2, &data, 0, HUMAN), "taint 0");
    assert!(
        words.contains("an after-image never lowers its row's taint"),
        "{words}"
    );
    assert_eq!(
        raw_image(&c, &id, 2, &data, 1, HUMAN).ok(),
        Some(1),
        "taint 1 is accepted"
    );
}

/// R5: an after-image's actor is a JSON object with a kind: `{}`, `"seat"`
/// and `[1]` are refused (a person's actor is accepted, the control), and a
/// seat's kind without the seat's provenance is refused too.
#[test]
fn an_after_images_actor_is_an_object_with_a_kind() {
    let r = rig();
    let c = r.raw();
    for (n, actor) in ["{}", "\"seat\"", "[1]", r#"{"kind":"seat"}"#]
        .into_iter()
        .enumerate()
    {
        let row = format!("a-row-by-hand-{n}");
        let result = raw_image(&c, &row, 1, r#"{"ref":"x"}"#, 0, actor);
        assert!(result.is_err(), "actor {actor}: refused, got {result:?}");
    }
    assert_eq!(
        raw_image(&c, "a-row-by-a-person", 1, r#"{"ref":"x"}"#, 0, HUMAN).ok(),
        Some(1),
        "a person's actor"
    );
}

/// R6: an attempt's seat and table are set only when it starts (queued to
/// running). On attempts that journaled none (started by `StartStep`), one
/// running and one finished, a raw update of `seat` or `writes_table` is
/// aborted with "an attempt's writes are set only when it starts" (no
/// other guard applies: nothing was set).
#[test]
fn an_attempts_writes_are_set_only_when_it_starts() {
    let r = rig();
    let running = r.latest(&r.run, "collect").id;
    r.exec(WriteCommand::StartStep {
        step_run_id: running.clone(),
    })
    .unwrap();
    let done = r.latest(&r.run, "check").id;
    r.exec(WriteCommand::StartStep {
        step_run_id: done.clone(),
    })
    .unwrap();
    r.exec(WriteCommand::FinishStep {
        step_run_id: done.clone(),
        status: "succeeded".into(),
        output: Some("done".into()),
    })
    .unwrap();
    assert_eq!(r.step(&done).status, "succeeded");
    let before = r.state();
    let c = r.raw();

    for id in [&running, &done] {
        for sql in [
            "UPDATE step_runs SET seat = 'seat-late' WHERE id = ?1",
            "UPDATE step_runs SET writes_table = 'entries' WHERE id = ?1",
        ] {
            let words = aborted_with(c.execute(sql, params![id]), sql);
            assert!(
                words.contains("an attempt's writes are set only when it starts"),
                "{sql} on {id}: {words}"
            );
        }
    }
    drop(c);
    assert_eq!(r.state(), before);
    assert_eq!(r.reader().attempt_writes(&running).unwrap(), None);
}

/// R7: a blank seat is journaled as none: an attempt started with
/// `AttemptWrites { seat: "" }` (the spike's linear workflow runs no seat)
/// has a NULL `step_runs.seat`, and `attempt_writes` gives `None`.
#[test]
fn a_blank_seat_is_journaled_as_none() {
    let r = rig();
    let a = r
        .start_with(
            &r.run,
            "collect",
            AttemptWrites {
                seat: String::new(),
                table: None,
                document: None,
            },
            Taint::Clean,
            vec![],
            vec![],
        )
        .unwrap();

    assert_eq!(r.reader().attempt_writes(&a.id).unwrap(), None);
    let seat: Option<String> = r
        .raw()
        .query_row(
            "SELECT seat FROM step_runs WHERE id = ?1",
            params![a.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(seat, None, "NULL, not an empty seat");
}

/// R8: a row is never written into a run that is no longer open. With the
/// attempt still `running` and its `table_write` call `issued`, a run
/// finished `cancelled`, `succeeded` or `failed` refuses the write in words
/// naming the run; nothing changes and the call stays `issued`.
#[test]
fn a_row_is_never_written_into_a_finished_run() {
    for status in ["cancelled", "succeeded", "failed"] {
        let r = rig();
        let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
        let call = r.issue(&a, 1);
        r.exec(WriteCommand::FinishRun {
            run_id: r.run.id.clone(),
            status: status.into(),
        })
        .unwrap();
        assert_eq!(r.step(&a.id).status, "running", "{status}");
        let before = r.state();

        let refused = r
            .write_at(
                &a,
                1,
                &call,
                insert_op(Uuid::now_v7(), &json!({ "ref": "r-1" })),
            )
            .expect_err("the run is not open");

        let words = refused.to_string();
        assert!(
            words.contains(&r.run.id),
            "{status}: names the run: {words}"
        );
        assert_eq!(r.state(), before, "{status}: nothing changed");
        assert_eq!(r.tool_call(&a, &call).status, "issued", "{status}");
    }
}

/// R9: a row's data is stored as the seat wrote it: a big integer and
/// `1e2` keep their text in the row and its after-image, and an update
/// merging `{"y":2}` keeps the big integer's text exactly.
#[test]
fn numbers_are_stored_as_written() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let mut ids = Vec::new();
    for (seq, text) in [
        (1, r#"{"ref":12345678901234567890123}"#),
        (2, r#"{"x":1e2}"#),
    ] {
        let call = r.issue(&a, seq);
        let id = Uuid::now_v7();
        r.write_at(
            &a,
            seq,
            &call,
            RowOp::Insert {
                row_id: id,
                data_json: text.into(),
            },
        )
        .unwrap();
        let id = id.to_string();
        assert_eq!(r.row(&id).data, text, "the row, as written");
        assert_eq!(r.history(&id)[0].data, text, "its after-image");
        ids.push(id);
    }
    let call = r.issue(&a, 3);
    r.write_at(
        &a,
        3,
        &call,
        RowOp::Update {
            row_id: Uuid::parse_str(&ids[0]).unwrap(),
            expected_version: 1,
            patch_json: r#"{"y":2}"#.into(),
        },
    )
    .unwrap();
    let merged = r.row(&ids[0]).data;
    assert!(
        merged.contains(r#""ref":12345678901234567890123"#),
        "the big integer keeps its text: {merged}"
    );
    assert!(merged.contains(r#""y":2"#), "{merged}");
    assert_eq!(r.history(&ids[0])[1].data, merged);
}

/// R10: a row over the byte cap (`am_store::ROW_MAX_BYTES`, its stored
/// JSON text) is refused in words, by insert and by an update that grows
/// it; a row at the cap is written (the boundary). Nothing else changes,
/// and the refusal does not echo the data.
#[test]
fn a_row_over_the_byte_cap_is_refused() {
    let cap = am_store::ROW_MAX_BYTES;
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let sized = |n: usize, fill: char| {
        let text = format!(r#"{{"ref":"{}"}}"#, fill.to_string().repeat(n - 10));
        assert_eq!(text.len(), n);
        text
    };
    let insert = |seq: i64, text: String| {
        let call = r.issue(&a, seq);
        let id = Uuid::now_v7();
        let result = r.write_at(
            &a,
            seq,
            &call,
            RowOp::Insert {
                row_id: id,
                data_json: text,
            },
        );
        (id, call, result)
    };

    let (at_cap, _, result) = insert(1, sized(cap, 'a'));
    result.expect("a row of exactly the cap is written");
    assert_eq!(r.row(&at_cap.to_string()).data.len(), cap);

    let before = r.state();
    let (_, call, result) = insert(2, sized(cap + 1, 'q'));
    let words = result.expect_err("one byte over").to_string();
    assert!(words.contains("byte"), "{words}");
    assert!(!words.contains("qqqqqqqq"), "no data echoed: {words}");
    assert_eq!(r.tool_call(&a, &call).status, "issued");
    let issued = r.state();
    assert_eq!(issued.rows, before.rows);
    assert_eq!(issued.history, before.history);

    let (small, _, result) = insert(3, sized(cap - 20, 'b'));
    result.unwrap();
    let call = r.issue(&a, 4);
    let grown = r.write_at(
        &a,
        4,
        &call,
        RowOp::Update {
            row_id: small,
            expected_version: 1,
            patch_json: format!(r#"{{"title":"{}"}}"#, "c".repeat(40)),
        },
    );
    let words = grown.expect_err("an update past the cap").to_string();
    assert!(words.contains("byte"), "{words}");
    assert_eq!(r.row(&small.to_string()).version, 1, "nothing changed");
}

/// R10: a row over the column cap (`am_store::ROW_MAX_COLUMNS`) is refused
/// in words, by insert and by an update that adds a column; a row at the
/// cap is written (the boundary).
#[test]
fn a_row_over_the_column_cap_is_refused() {
    let cap = am_store::ROW_MAX_COLUMNS;
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let columns = |n: usize| {
        let cells: Vec<String> = (0..n).map(|i| format!(r#""c{i}":{i}"#)).collect();
        format!("{{{}}}", cells.join(","))
    };

    let call = r.issue(&a, 1);
    let at_cap = Uuid::now_v7();
    r.write_at(
        &a,
        1,
        &call,
        RowOp::Insert {
            row_id: at_cap,
            data_json: columns(cap),
        },
    )
    .expect("a row of exactly the cap is written");

    let before = r.state();
    let call = r.issue(&a, 2);
    let over = r.write_at(
        &a,
        2,
        &call,
        RowOp::Insert {
            row_id: Uuid::now_v7(),
            data_json: columns(cap + 1),
        },
    );
    let words = over.expect_err("one column over").to_string();
    assert!(words.contains("column"), "{words}");
    let after = r.state();
    assert_eq!((after.rows, after.history), (before.rows, before.history));

    let call = r.issue(&a, 3);
    let added = r.write_at(
        &a,
        3,
        &call,
        RowOp::Update {
            row_id: at_cap,
            expected_version: 1,
            patch_json: r#"{"one_more":1}"#.into(),
        },
    );
    let words = added.expect_err("an update past the cap").to_string();
    assert!(words.contains("column"), "{words}");
    assert_eq!(r.row(&at_cap.to_string()).version, 1, "nothing changed");
}

/// R11: `RowOp` and `RowWrite` print no row data: their `Debug` holds
/// neither a column name nor a value of the data or the patch, only counts
/// and lengths (the data's byte length is there).
#[test]
fn a_row_writes_debug_holds_no_data() {
    let data = r#"{"column_canary_7d21":"value-canary-7d22"}"#;
    let patch = r#"{"title":"patch-canary-7d23"}"#;
    let a = Attempt {
        run: Run {
            id: "run-1".into(),
            uuid: Uuid::nil(),
        },
        key: "collect".into(),
        id: "step-run-1".into(),
    };
    let insert = RowOp::Insert {
        row_id: Uuid::nil(),
        data_json: data.into(),
    };
    let update = RowOp::Update {
        row_id: Uuid::nil(),
        expected_version: 1,
        patch_json: patch.into(),
    };
    for (op, text) in [(insert, data), (update, patch)] {
        let write = row_write(&a, 1, "tool-call-1", TABLE, op.clone(), Taint::Clean);
        for shown in [format!("{op:?}"), format!("{write:?}")] {
            for canary in [
                "column_canary_7d21",
                "value-canary-7d22",
                "patch-canary-7d23",
                "title",
            ] {
                assert!(!shown.contains(canary), "`{canary}` in {shown}");
            }
            assert!(
                shown.contains(&text.len().to_string()),
                "the byte length {}: {shown}",
                text.len()
            );
        }
    }
}

/// R12: a refusal never repeats the caller's table id. A write naming a
/// table that is not the attempt's (a canary) is refused in words that name
/// the attempt's journaled table and not the canary; so is one into an
/// attempt that declares no table.
#[test]
fn a_wrong_tables_refusal_never_echoes_its_id() {
    const CANARY: &str = "table-canary-0c41";
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let call = r.issue(&a, 1);
    let fresh = || insert_op(Uuid::now_v7(), &json!({ "ref": "r-1" }));

    let words = r
        .apply(
            a.actor(),
            row_write(&a, 1, &call, CANARY, fresh(), Taint::Clean),
        )
        .expect_err("another table")
        .to_string();
    assert!(!words.contains(CANARY), "{words}");
    assert!(words.contains(TABLE), "names the journaled table: {words}");

    let b = r.begin(&r.run, "check", None, None, Taint::Clean);
    let b_call = r.issue(&b, 1);
    let words = r
        .apply(
            b.actor(),
            row_write(&b, 1, &b_call, CANARY, fresh(), Taint::Clean),
        )
        .expect_err("no table")
        .to_string();
    assert!(!words.contains(CANARY), "{words}");
}

/// R2b (review triage round 1): a tool call's `id`, `step_run_id`,
/// `call_seq` and `tool` are fixed for every call, with a result kind or
/// not (finishing a call sets only its status and result). The reverse
/// collision, an `UPDATE OR REPLACE` of an unkinded call onto a kinded
/// call's sequence, which would delete the kinded call, is aborted, and the
/// kinded call stays exactly as it was; a plain update of any of the four
/// on an unkinded, issued call is aborted too.
#[test]
fn a_tool_calls_identity_never_changes() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    r.insert(&a, 1, &json!({ "ref": "r-1" }));
    let kinded = r.reader().tool_calls(&a.id).unwrap()[0].clone();
    assert_eq!(kinded.result_kind.as_deref(), Some("engine"));
    let unkinded = r.issue(&a, 2);
    let elsewhere = r.latest(&r.run, "check").id;
    let before = r.state();
    let c = r.raw();

    aborted_with(
        c.execute(
            "UPDATE OR REPLACE tool_calls SET call_seq = ?2 WHERE id = ?1",
            params![unkinded, kinded.call_seq],
        ),
        "the reverse collision",
    );
    for (what, sql) in [
        (
            "id",
            "UPDATE tool_calls SET id = 'a-forged-call-id' WHERE id = ?1",
        ),
        (
            "step_run_id",
            "UPDATE tool_calls SET step_run_id = ?2 WHERE id = ?1",
        ),
        (
            "call_seq",
            "UPDATE tool_calls SET call_seq = 99 WHERE id = ?1",
        ),
        (
            "tool",
            "UPDATE tool_calls SET tool = 'other_tool' WHERE id = ?1",
        ),
    ] {
        let result = if sql.contains("?2") {
            c.execute(sql, params![unkinded, elsewhere])
        } else {
            c.execute(sql, params![unkinded])
        };
        aborted_with(result, what);
    }
    drop(c);
    assert_eq!(r.state(), before, "every call as it was");
    assert_eq!(
        r.tool_call(&a, &kinded.id),
        kinded,
        "the kinded call, exactly as it was"
    );
    assert_eq!(r.tool_call(&a, &unkinded).status, "issued");
}

// --------------------------------------------- review triage round 2 (T1–T6)
//
// The manager's triage of the follow-up review (2026-11-24, the store
// plan's "Review triage round 2"). Written before the fixes.

/// T1 (a): an attempt is never replaced. A started, tainted attempt that
/// journaled `seat-writer` and `entries` (and has no child row, so no
/// foreign key stands in the way) is re-inserted with `INSERT OR REPLACE`
/// under another seat and table and a lower taint: by its id, by its rowid
/// (under a new id) and by its (run, step, attempt) (under a new id). Each
/// is aborted with "an attempt is never replaced", and the attempt keeps
/// its writes and its taint. The other guards on `step_runs` are set aside
/// in this test's database (round 4: U3's insert guard also refuses these
/// rows, and SQLite fires the trigger created last first), so the words are
/// this guard's.
#[test]
fn an_attempt_is_never_replaced() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Tainted);
    let before = r.state();
    let c = r.raw();
    leave_only(&c, "step_runs", "an attempt is never replaced");

    for (what, sql) in [
        (
            "by its id",
            "INSERT OR REPLACE INTO step_runs (id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at, seat, writes_table)
             SELECT id, run_id, step_key, seq, attempt, status, output, 0, updated_at, started_at, finished_at, 'seat-other', 'others' FROM step_runs WHERE id = ?1",
        ),
        (
            "by its rowid",
            "INSERT OR REPLACE INTO step_runs (rowid, id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at, seat, writes_table)
             SELECT rowid, 'a-new-attempt-id', run_id, step_key, seq, 99, status, output, 0, updated_at, started_at, finished_at, 'seat-other', 'others' FROM step_runs WHERE id = ?1",
        ),
        (
            "by its run, step and attempt",
            "INSERT OR REPLACE INTO step_runs (id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at, seat, writes_table)
             SELECT 'a-new-attempt-id', run_id, step_key, seq, attempt, status, output, 0, updated_at, started_at, finished_at, 'seat-other', 'others' FROM step_runs WHERE id = ?1",
        ),
    ] {
        let words = aborted_with(c.execute(sql, params![a.id]), what);
        assert!(
            words.contains("an attempt is never replaced"),
            "{what}: {words}"
        );
    }
    drop(c);
    assert_eq!(r.state(), before, "every attempt as it was");
    assert_eq!(
        r.reader().attempt_writes(&a.id).unwrap(),
        Some(writes(Some(TABLE), None))
    );
    assert_eq!(r.step(&a.id).taint, Taint::Tainted);
}

/// T1 (b), as the manager decided it (2026-11-24): an attempt never goes
/// back to `queued`, so a status round trip can never reopen its writes. A
/// raw update moving a started attempt's status back to `queued` is aborted
/// with "an attempt never goes back to queued", from `running` (with its
/// writes journaled, and started by `StartStep` with none) and from a
/// finished status (`succeeded`, `failed`). Every attempt, its seat and its
/// table stay as they were. A queued attempt still starts as before and
/// journals its writes.
#[test]
fn a_status_round_trip_never_reopens_an_attempts_writes() {
    let r = rig_with(&["collect", "check", "plain", "more", "last"]);
    let started = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let bare = r.latest(&r.run, "check").id;
    r.exec(WriteCommand::StartStep {
        step_run_id: bare.clone(),
    })
    .unwrap();
    let done = r.begin(&r.run, "plain", Some(TABLE), None, Taint::Clean);
    r.finish(&done);
    let failed = r.latest(&r.run, "more").id;
    r.exec(WriteCommand::StartStep {
        step_run_id: failed.clone(),
    })
    .unwrap();
    r.exec(WriteCommand::FinishStep {
        step_run_id: failed.clone(),
        status: "failed".into(),
        output: None,
    })
    .unwrap();
    let before = r.state();
    let c = r.raw();

    for (what, id) in [
        ("running, with writes", &started.id),
        ("running, started with none", &bare),
        ("succeeded", &done.id),
        ("failed", &failed),
    ] {
        let words = aborted_with(
            c.execute(
                "UPDATE step_runs SET status = 'queued' WHERE id = ?1",
                params![id],
            ),
            what,
        );
        assert!(
            words.contains("an attempt never goes back to queued"),
            "{what}: {words}"
        );
    }
    drop(c);
    assert_eq!(r.state(), before, "every attempt as it was");
    assert_eq!(
        r.reader().attempt_writes(&started.id).unwrap(),
        Some(writes(Some(TABLE), None))
    );
    assert_eq!(r.reader().attempt_writes(&bare).unwrap(), None);
    assert_eq!(
        r.reader().attempt_writes(&done.id).unwrap(),
        Some(writes(Some(TABLE), None))
    );

    let next = r.begin(&r.run, "last", Some(TABLE), None, Taint::Clean);
    assert_eq!(
        r.step(&next.id).status,
        "running",
        "a queued attempt starts"
    );
    assert_eq!(
        r.reader().attempt_writes(&next.id).unwrap(),
        Some(writes(Some(TABLE), None))
    );
}

/// T2: a key appears once in a row: an insert whose data repeats a key at
/// the top level, or inside a nested object, and an update whose patch
/// repeats one, are each refused in words that say so (`duplicate`) and
/// never name the key; nothing changes and the call stays `issued`.
#[test]
fn a_repeated_key_is_refused_in_words() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let id = r.insert(&a, 1, &json!({ "ref": "r-1" }));
    let row = Uuid::parse_str(&id).unwrap();

    for (seq, what, op) in [
        (
            2,
            "a repeated top-level key",
            RowOp::Insert {
                row_id: Uuid::now_v7(),
                data_json: r#"{"key_canary_3e10":"a","key_canary_3e10":"b"}"#.into(),
            },
        ),
        (
            3,
            "a repeated nested key",
            RowOp::Insert {
                row_id: Uuid::now_v7(),
                data_json: r#"{"ref":"r-2","extra":{"key_canary_3e10":1,"key_canary_3e10":2}}"#
                    .into(),
            },
        ),
        (
            4,
            "a repeated key in a patch",
            RowOp::Update {
                row_id: row,
                expected_version: 1,
                patch_json: r#"{"key_canary_3e10":"x","key_canary_3e10":"y"}"#.into(),
            },
        ),
    ] {
        let call = r.issue(&a, seq);
        let before = r.state();
        let words = r.write_at(&a, seq, &call, op).expect_err(what).to_string();
        assert!(words.contains("duplicate"), "{what}: {words}");
        assert!(!words.contains("key_canary_3e10"), "{what}: {words}");
        assert_eq!(r.state(), before, "{what}: nothing changed");
        assert_eq!(r.tool_call(&a, &call).status, "issued", "{what}");
    }
    assert_eq!(r.row(&id).version, 1);
}

/// T3: a patch is checked against the caps before it is merged. A patch
/// over `ROW_MAX_BYTES`, and one with more keys than `ROW_MAX_COLUMNS`, are
/// refused in words (`byte`, `column`); a large patch (about half a
/// megabyte, tens of thousands of keys) is refused in well under the
/// generous bound of two seconds, on the single writer thread; nothing
/// changes.
#[test]
fn an_oversized_patch_is_refused_before_it_is_merged() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let id = r.insert(&a, 1, &json!({ "ref": "r-1" }));
    let row = Uuid::parse_str(&id).unwrap();
    let keys = |n: usize| {
        let cells: Vec<String> = (0..n).map(|i| format!(r#""k{i}":0"#)).collect();
        format!("{{{}}}", cells.join(","))
    };
    let big_value = format!(r#"{{"title":"{}"}}"#, "x".repeat(am_store::ROW_MAX_BYTES));
    let many = keys(am_store::ROW_MAX_COLUMNS + 1);
    let large = keys(50_000);
    assert!(large.len() > am_store::ROW_MAX_BYTES);

    for (seq, what, patch, word) in [
        (2, "over the byte cap", big_value, "byte"),
        (3, "over the column cap", many, "column"),
        (4, "a large patch", large, "byte"),
    ] {
        let call = r.issue(&a, seq);
        let before = r.state();
        let started = std::time::Instant::now();
        let result = r.write_at(
            &a,
            seq,
            &call,
            RowOp::Update {
                row_id: row,
                expected_version: 1,
                patch_json: patch,
            },
        );
        let took = started.elapsed();
        let words = result.expect_err(what).to_string();
        assert!(
            words.contains(word) || (what == "a large patch" && words.contains("column")),
            "{what}: {words}"
        );
        assert!(
            took < std::time::Duration::from_secs(2),
            "{what}: refused before the merge, in {took:?}"
        );
        assert_eq!(r.state(), before, "{what}: nothing changed");
    }
    assert_eq!(r.row(&id).version, 1);
}

/// T4: a tool call is never replaced by its rowid. An `INSERT OR REPLACE`
/// of a new call at the rowid of a call whose kind is set (an `untrusted`
/// call, which no after-image references, so no foreign key stands in the
/// way) is aborted with "a tool call is never replaced", and the kinded
/// call stays exactly as it was.
#[test]
fn a_kinded_call_is_never_replaced_by_its_rowid() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let finished = r.record(&a, 1, "fixture_read", "issued");
    r.exec(WriteCommand::FinishToolCall {
        tool_call_id: finished.clone(),
        status: "succeeded".into(),
    })
    .unwrap();
    let kinded = r.tool_call(&a, &finished);
    assert_eq!(kinded.result_kind.as_deref(), Some("untrusted"));
    let before = r.state();
    let c = r.raw();

    let words = aborted_with(
        c.execute(
            "INSERT OR REPLACE INTO tool_calls (rowid, id, step_run_id, call_seq, tool, status, created_at)
             SELECT rowid, 'a-new-call-id', step_run_id, 77, 'other_tool', 'issued', created_at FROM tool_calls WHERE id = ?1",
            params![finished],
        ),
        "a replace by rowid",
    );
    assert!(words.contains("a tool call is never replaced"), "{words}");
    drop(c);
    assert_eq!(r.state(), before);
    assert_eq!(
        r.tool_call(&a, &finished),
        kinded,
        "the kinded call, as it was"
    );
}

/// T5: a replay is only the write a key already holds. A seat's after-image
/// planted by raw SQL at a key whose `table_write` call is still `issued`
/// (no kind) makes no replay: `apply_row_write` at that key is refused in
/// words (not `Ok`), and the call stays `issued` with no kind.
#[test]
fn a_planted_after_image_is_no_replay() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let call = r.issue(&a, 1);
    let actor = serde_json::to_string(&a.actor()).unwrap();
    let c = r.raw();
    c.execute(
        "INSERT INTO row_history (table_id, row_id, version, data, taint, actor, run_id, step_run_id, seat, call_seq, tool_call_id, op, created_at)
         VALUES (?1, ?2, 1, '{\"ref\":\"planted\"}', 0, ?3, ?4, ?5, ?6, 1, ?7, 'insert', '2026-11-24T10:00:00.000Z')",
        params![TABLE, Uuid::now_v7().to_string(), actor, r.run.id, a.id, SEAT, call],
    )
    .expect("a seat's after-image planted by hand");
    drop(c);
    let before = r.state();

    let result = r.write_at(
        &a,
        1,
        &call,
        insert_op(Uuid::now_v7(), &json!({ "ref": "r-1" })),
    );

    let words = result
        .expect_err("a planted after-image is no replay")
        .to_string();
    assert!(!words.trim().is_empty());
    assert_eq!(r.state(), before, "nothing changed");
    let row = r.tool_call(&a, &call);
    assert_eq!((row.status.as_str(), row.result_kind), ("issued", None));
}

/// T6: no row type prints a row's data or its table id. The `Debug` of
/// `RowWrite`, `RowOp`, `RowRecord` and `RowHistoryRecord` holds no canary
/// planted in the data and in the table id.
#[test]
fn no_row_types_debug_holds_data_or_a_table_id() {
    const TABLE_CANARY: &str = "table-canary-5a91";
    const DATA_CANARY: &str = "data-canary-5a92";
    let data = format!(r#"{{"ref":"{DATA_CANARY}"}}"#);
    let write = RowWrite {
        tool_call_id: "tool-call-1".into(),
        step_run_id: "step-run-1".into(),
        call_seq: 1,
        table_id: TABLE_CANARY.into(),
        op: RowOp::Insert {
            row_id: Uuid::nil(),
            data_json: data.clone(),
        },
        taint: Taint::Clean,
    };
    let record = RowRecord {
        table_id: TABLE_CANARY.into(),
        row_id: Uuid::nil().to_string(),
        data: data.clone(),
        version: 1,
        taint: Taint::Clean,
        created_at: "2026-11-24T10:00:00.000Z".into(),
        updated_at: "2026-11-24T10:00:00.000Z".into(),
    };
    let image = RowHistoryRecord {
        seq: 1,
        table_id: TABLE_CANARY.into(),
        row_id: Uuid::nil().to_string(),
        version: 1,
        data: data.clone(),
        taint: Taint::Clean,
        actor: you_reviewer(),
        run_id: None,
        step_run_id: None,
        seat: None,
        call_seq: None,
        tool_call_id: None,
        op: "insert".into(),
        created_at: "2026-11-24T10:00:00.000Z".into(),
    };
    for (what, shown) in [
        ("RowWrite", format!("{write:?}")),
        ("RowOp", format!("{:?}", write.op)),
        ("RowRecord", format!("{record:?}")),
        ("RowHistoryRecord", format!("{image:?}")),
    ] {
        for canary in [TABLE_CANARY, DATA_CANARY] {
            assert!(!shown.contains(canary), "{what}: `{canary}` in {shown}");
        }
    }
}

// ------------------------------------------------ the owner's round 3 (T1)
//
// The owner, 2026-11-24 (PR #38): the no-requeue guard keeps AD-8's one
// same-row edge back, `waiting_branches → queued` (a fan-in whose join is
// met, story 3.14), and nothing else: every other move to `queued` is
// refused; a move into `waiting_branches` comes only from `queued`; and
// `waiting_branches → queued` is refused for a row with a seat, a table or
// a `started_at`. Raw SQL reaches the guards (`step_runs.status` has no
// CHECK). Written before the fix.

/// Every status a step attempt leaves `queued` for, but `waiting_branches`.
const NOT_QUEUED: [&str; 7] = [
    "running",
    "waiting_human",
    "waiting_review",
    "succeeded",
    "failed",
    "cancelled",
    "looped_back",
];

/// A fresh queued attempt of a new run's one step `s`: its id.
fn fresh_attempt(r: &Rig) -> String {
    let run = r.new_run(&["s"]);
    r.latest(&run, "s").id
}

/// Moves `id`'s status to `status` by hand (the test's setup; it must go
/// through).
fn set_status(c: &Connection, id: &str, status: &str) {
    let changed = c
        .execute(
            "UPDATE step_runs SET status = ?2 WHERE id = ?1",
            params![id, status],
        )
        .unwrap_or_else(|e| panic!("setting up {status}: {e}"));
    assert_eq!(changed, 1, "setting up {status}");
}

/// A raw attempt row of a new run's step `s` (attempt 2), already
/// `waiting_branches`, with the given seat, table and start (a row only a
/// database with the insert guard set aside holds, round 4: U3).
fn waiting_row(
    r: &Rig,
    c: &Connection,
    seat: Option<&str>,
    table: Option<&str>,
    started_at: Option<&str>,
) -> String {
    let run = r.new_run(&["s"]);
    let id = new_id();
    c.execute(
        "INSERT INTO step_runs (id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at, seat, writes_table)
         VALUES (?1, ?2, 's', 0, 2, 'waiting_branches', NULL, 0, '2026-11-24T10:00:00.000Z', ?3, NULL, ?4, ?5)",
        params![id, run.id, started_at, seat, table],
    )
    .expect("a waiting_branches attempt made by hand");
    id
}

/// AD-8's fan-in edges are allowed: a fresh queued attempt moves to
/// `waiting_branches` (its join not met) and back to `queued` (its join
/// met), having no seat, no table and no `started_at`; it then starts as
/// any queued attempt does and journals its writes.
#[test]
fn the_fan_in_edges_of_ad_8_are_allowed() {
    let r = rig();
    let id = r.latest(&r.run, "collect").id;
    let c = r.raw();

    set_status(&c, &id, "waiting_branches");
    let back = c.execute(
        "UPDATE step_runs SET status = 'queued' WHERE id = ?1",
        params![id],
    );
    assert_eq!(
        back.as_ref().ok(),
        Some(&1),
        "waiting_branches → queued, the join met: {back:?}"
    );
    drop(c);

    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    assert_eq!(a.id, id);
    assert_eq!(r.step(&id).status, "running");
    assert_eq!(
        r.reader().attempt_writes(&id).unwrap(),
        Some(writes(Some(TABLE), None))
    );
}

/// No status but `waiting_branches` goes back to `queued`: from each of
/// `running`, `waiting_human`, `waiting_review`, `succeeded`, `failed`,
/// `cancelled` and `looped_back`, a raw move to `queued` is aborted with "an
/// attempt never goes back to queued", and the attempt stays as it was.
#[test]
fn no_status_but_waiting_branches_goes_back_to_queued() {
    let r = rig();
    let c = r.raw();
    for status in NOT_QUEUED {
        let id = fresh_attempt(&r);
        set_status(&c, &id, status);
        let before = r.step(&id);

        let words = aborted_with(
            c.execute(
                "UPDATE step_runs SET status = 'queued' WHERE id = ?1",
                params![id],
            ),
            status,
        );

        assert!(
            words.contains("an attempt never goes back to queued"),
            "{status}: {words}"
        );
        assert_eq!(r.step(&id), before, "{status}: as it was");
    }
}

/// An attempt waits for branches only from `queued` (AD-8's one edge in):
/// from each of `running`, `waiting_human`, `waiting_review`, `succeeded`,
/// `failed`, `cancelled` and `looped_back`, a raw move to
/// `waiting_branches` is aborted with "an attempt waits for branches only
/// from queued", and the attempt stays as it was.
#[test]
fn only_a_queued_attempt_waits_for_branches() {
    let r = rig();
    let c = r.raw();
    for status in NOT_QUEUED {
        let id = fresh_attempt(&r);
        set_status(&c, &id, status);
        let before = r.step(&id);

        let words = aborted_with(
            c.execute(
                "UPDATE step_runs SET status = 'waiting_branches' WHERE id = ?1",
                params![id],
            ),
            status,
        );

        assert!(
            words.contains("an attempt waits for branches only from queued"),
            "{status}: {words}"
        );
        assert_eq!(r.step(&id), before, "{status}: as it was");
    }
}

/// `waiting_branches → queued` is only for an attempt that never started:
/// a waiting row with a seat, with a table, or with a `started_at` (one
/// case each) is refused the move with "only an attempt that never started
/// goes back to queued", and stays `waiting_branches`. Round 4 (U3)
/// refuses inserting such a row, so the test sets every other guard on
/// `step_runs` aside in its own database to make them.
#[test]
fn a_started_attempt_never_goes_back_from_waiting_branches() {
    let r = rig();
    let c = r.raw();
    leave_only(
        &c,
        "step_runs",
        "only an attempt that never started goes back to queued",
    );
    for (what, seat, table, started_at) in [
        ("a seat", Some(SEAT), None, None),
        ("a table", None, Some(TABLE), None),
        ("a start", None, None, Some("2026-11-24T10:00:00.000Z")),
    ] {
        let id = waiting_row(&r, &c, seat, table, started_at);

        let words = aborted_with(
            c.execute(
                "UPDATE step_runs SET status = 'queued' WHERE id = ?1",
                params![id],
            ),
            what,
        );

        assert!(
            words.contains("only an attempt that never started goes back to queued"),
            "{what}: {words}"
        );
        assert_eq!(r.step(&id).status, "waiting_branches", "{what}");
    }
}

/// The round trip `running → waiting_branches → queued` is refused at its
/// first step: a started attempt with its writes journaled is refused the
/// move to `waiting_branches` with "an attempt waits for branches only from
/// queued"; the move to `queued` is refused too; the attempt, its seat and
/// its table stay as they were.
#[test]
fn the_round_trip_through_waiting_branches_is_refused() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let before = r.state();
    let c = r.raw();

    let words = aborted_with(
        c.execute(
            "UPDATE step_runs SET status = 'waiting_branches' WHERE id = ?1",
            params![a.id],
        ),
        "running → waiting_branches",
    );
    assert!(
        words.contains("an attempt waits for branches only from queued"),
        "{words}"
    );
    aborted_with(
        c.execute(
            "UPDATE step_runs SET status = 'queued' WHERE id = ?1",
            params![a.id],
        ),
        "running → queued",
    );
    drop(c);

    assert_eq!(r.state(), before, "the attempt as it was");
    assert_eq!(r.step(&a.id).status, "running");
    assert_eq!(
        r.reader().attempt_writes(&a.id).unwrap(),
        Some(writes(Some(TABLE), None))
    );
}

// ------------------------------------------- review triage round 4 (U1–U3)
//
// The manager's triage of the security re-review (2026-11-24, the store
// plan's "Review triage round 4"): an attempt's identity never changes
// (U1), its `started_at` is set once (U2), and a new attempt is inserted
// only as `queued`, with no seat, table or start (U3). Raw SQL reaches the
// guards. Written before the fixes.

/// Every cell of attempt `id`, its rowid first; `None` when it is gone.
fn attempt_cells(c: &Connection, id: &str) -> Option<Vec<Cell>> {
    let mut st = c
        .prepare("SELECT rowid, * FROM step_runs WHERE id = ?1")
        .unwrap();
    let n = st.column_count();
    st.query_map(params![id], |r| {
        (0..n)
            .map(|i| r.get::<_, Cell>(i))
            .collect::<rusqlite::Result<Vec<Cell>>>()
    })
    .unwrap()
    .next()
    .map(|cells| cells.unwrap())
}

/// U1, the exact swap: a fresh queued attempt B takes a finished attempt
/// A's identity with `UPDATE OR REPLACE` (A's id, rowid, step, sequence and
/// attempt), which would delete A and leave B, with taint 0 and no writes,
/// owning A's tool calls and after-images. A is `succeeded`, tainted, with
/// a seat and a table, a tool call and an after-image. The swap is aborted
/// with "an attempt's identity never changes"; A and B are as they were,
/// and so is every table.
#[test]
fn the_identity_swap_onto_a_finished_attempt_aborts() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Tainted);
    let row = r.insert(&a, 1, &json!({ "ref": "r-1" }));
    r.finish(&a);
    let b = r.latest(&r.run, "check").id;
    let a_row = r.step(&a.id);
    assert_eq!(a_row.status, "succeeded", "setup: A finished");
    assert_eq!(a_row.taint, Taint::Tainted, "setup: A tainted");
    assert_eq!(
        r.reader().attempt_writes(&a.id).unwrap(),
        Some(writes(Some(TABLE), None)),
        "setup: A has a seat and a table"
    );
    assert_eq!(r.reader().tool_calls(&a.id).unwrap().len(), 1, "setup");
    assert_eq!(r.history(&row).len(), 1, "setup: A's after-image");
    assert_eq!(r.step(&b).status, "queued", "setup: B is fresh");
    let c = r.raw();
    let rowid: i64 = c
        .query_row(
            "SELECT rowid FROM step_runs WHERE id = ?1",
            params![a.id],
            |x| x.get(0),
        )
        .unwrap();
    let (a_cells, b_cells) = (attempt_cells(&c, &a.id), attempt_cells(&c, &b));
    let before = tables_of(&r.db);

    let words = aborted_with(
        c.execute(
            &format!(
                "UPDATE OR REPLACE step_runs SET id = '{}', rowid = {rowid}, step_key = '{}', seq = {}, attempt = {} WHERE id = '{b}'",
                a.id, a_row.step_key, a_row.seq, a_row.attempt
            ),
            [],
        ),
        "the identity swap",
    );

    assert!(
        words.contains("an attempt's identity never changes"),
        "{words}"
    );
    assert_eq!(attempt_cells(&c, &a.id), a_cells, "A, cell for cell");
    assert_eq!(attempt_cells(&c, &b), b_cells, "B, cell for cell");
    drop(c);
    assert_eq!(tables_of(&r.db), before, "every table as it was");
    assert_eq!(r.step(&a.id), a_row);
    assert_eq!(
        r.reader().attempt_writes(&a.id).unwrap(),
        Some(writes(Some(TABLE), None))
    );
    assert_eq!(r.reader().tool_calls(&a.id).unwrap().len(), 1);
    assert_eq!(
        r.history(&row)[0].step_run_id.as_deref(),
        Some(a.id.as_str())
    );
}

/// U1, one column at a time: on a fresh queued attempt, a raw change of its
/// `id`, its rowid, its `run_id` (to another run that exists), its
/// `step_key`, its `seq` or its `attempt` is each aborted with "an
/// attempt's identity never changes", and the attempt stays as it was. An
/// update that names none of them goes through (the control), and the
/// attempt then starts and finishes as any does.
#[test]
fn an_attempts_identity_never_changes() {
    let r = rig();
    let run = r.new_run(&["s"]);
    let id = r.latest(&run, "s").id;
    let c = r.raw();
    let before = attempt_cells(&c, &id);

    for (what, sql) in [
        (
            "id",
            "UPDATE step_runs SET id = 'a-forged-attempt-id' WHERE id = ?1",
        ),
        (
            "rowid",
            "UPDATE step_runs SET rowid = (SELECT MAX(rowid) + 100 FROM step_runs) WHERE id = ?1",
        ),
        ("run_id", "UPDATE step_runs SET run_id = ?2 WHERE id = ?1"),
        (
            "step_key",
            "UPDATE step_runs SET step_key = 'another-step' WHERE id = ?1",
        ),
        ("seq", "UPDATE step_runs SET seq = 99 WHERE id = ?1"),
        ("attempt", "UPDATE step_runs SET attempt = 99 WHERE id = ?1"),
    ] {
        let result = if sql.contains("?2") {
            c.execute(sql, params![id, r.run.id])
        } else {
            c.execute(sql, params![id])
        };
        let words = aborted_with(result, what);
        assert!(
            words.contains("an attempt's identity never changes"),
            "{what}: {words}"
        );
        assert_eq!(attempt_cells(&c, &id), before, "{what}: as it was");
    }

    let control = c.execute(
        "UPDATE step_runs SET updated_at = '2026-11-24T10:00:00.000Z' WHERE id = ?1",
        params![id],
    );
    assert_eq!(control.as_ref().ok(), Some(&1), "the control: {control:?}");
    drop(c);
    let a = r.begin(&run, "s", Some(TABLE), None, Taint::Clean);
    assert_eq!(a.id, id);
    r.finish(&a);
    assert_eq!(r.step(&id).status, "succeeded");
}

/// U2: an attempt's `started_at` is set once. The normal start sets it the
/// first time (`StartAttempt` with writes, `StartStep` with none, both from
/// `NULL`); after that a raw change of it, or a raw `NULL`, is each aborted
/// with "an attempt's start time is set once", on a running attempt (with
/// and without writes) and on a finished one, and the attempt stays as it
/// was.
#[test]
fn an_attempts_start_time_is_set_once() {
    let r = rig();
    for key in ["collect", "check", "plain"] {
        assert_eq!(r.latest(&r.run, key).started_at, None, "{key}: queued");
    }
    let with_writes = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let bare = r.latest(&r.run, "check").id;
    r.exec(WriteCommand::StartStep {
        step_run_id: bare.clone(),
    })
    .unwrap();
    let done = r.begin(&r.run, "plain", Some(TABLE), None, Taint::Clean);
    r.finish(&done);
    let c = r.raw();

    for (what, id) in [
        ("running, with writes", &with_writes.id),
        ("running, started with none", &bare),
        ("succeeded", &done.id),
    ] {
        let set = r.step(id);
        assert_timestamp(
            set.started_at
                .as_deref()
                .unwrap_or_else(|| panic!("{what}: the normal start sets it")),
        );
        for (how, sql) in [
            (
                "changed",
                "UPDATE step_runs SET started_at = '2030-01-01T00:00:00.000Z' WHERE id = ?1",
            ),
            (
                "nulled",
                "UPDATE step_runs SET started_at = NULL WHERE id = ?1",
            ),
        ] {
            let words = aborted_with(c.execute(sql, params![id]), &format!("{what}, {how}"));
            assert!(
                words.contains("an attempt's start time is set once"),
                "{what}, {how}: {words}"
            );
            assert_eq!(r.step(id), set, "{what}, {how}: as it was");
        }
    }
}

/// A raw insert of attempt 1 of step `key` of `run`, with `status`, seat,
/// table and start.
fn insert_attempt(
    c: &Connection,
    run: &str,
    key: &str,
    status: &str,
    seat: Option<&str>,
    table: Option<&str>,
    started_at: Option<&str>,
) -> rusqlite::Result<usize> {
    c.execute(
        "INSERT INTO step_runs (id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at, seat, writes_table)
         VALUES (?1, ?2, ?3, 0, 1, ?4, NULL, 0, '2026-11-24T10:00:00.000Z', ?5, NULL, ?6, ?7)",
        params![new_id(), run, key, status, started_at, seat, table],
    )
}

/// U3: a new attempt is inserted only as `queued`, with no seat, no table
/// and no `started_at`. A raw insert with any other status (each of
/// `running`, `waiting_human`, `waiting_review`, `succeeded`, `failed`,
/// `cancelled`, `looped_back` and `waiting_branches`), or a queued one with
/// a seat, with a table or with a `started_at`, is each aborted with "a new
/// attempt is inserted only as queued, with no seat, table or start", and
/// nothing changes. A plain queued insert goes through, and the store's own
/// two insert paths (a new run, a loop-back's next attempt) still queue
/// attempts.
#[test]
fn a_new_attempt_is_inserted_only_queued_and_unstarted() {
    let r = rig_with(&["collect", "check"]);
    let c = r.raw();
    let before = tables_of(&r.db);
    let refused = NOT_QUEUED
        .iter()
        .chain(["waiting_branches"].iter())
        .map(|status| (status.to_string(), *status, None, None, None))
        .chain([
            ("a seat".to_string(), "queued", Some(SEAT), None, None),
            ("a table".to_string(), "queued", None, Some(TABLE), None),
            (
                "a start".to_string(),
                "queued",
                None,
                None,
                Some("2026-11-24T10:00:00.000Z"),
            ),
        ]);

    for (i, (what, status, seat, table, started_at)) in refused.enumerate() {
        let words = aborted_with(
            insert_attempt(
                &c,
                &r.run.id,
                &format!("new-{i}"),
                status,
                seat,
                table,
                started_at,
            ),
            &what,
        );
        assert!(
            words
                .contains("a new attempt is inserted only as queued, with no seat, table or start"),
            "{what}: {words}"
        );
    }
    assert_eq!(tables_of(&r.db), before, "nothing inserted");

    let plain = insert_attempt(&c, &r.run.id, "new-plain", "queued", None, None, None);
    assert_eq!(
        plain.as_ref().ok(),
        Some(&1),
        "a plain queued insert: {plain:?}"
    );
    drop(c);

    let run = r.new_run(&["a", "b"]);
    for key in ["a", "b"] {
        let s = r.latest(&run, key);
        assert_eq!((s.status.as_str(), s.started_at), ("queued", None), "{key}");
        assert_eq!(r.reader().attempt_writes(&s.id).unwrap(), None, "{key}");
    }
    let first = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    r.finish(&first);
    r.loop_back(&r.run, "check", "collect");
    let next = r.latest(&r.run, "collect");
    assert_eq!((next.attempt, next.status.as_str()), (2, "queued"));
    assert_eq!(next.started_at, None);
    assert_eq!(r.reader().attempt_writes(&next.id).unwrap(), None);
}

// ------------------------------------- ToolGate slice, review round 1 (3.23)
//
// The ToolGate slice's review triage round 1 (manager, 2026-11-25):
// - R1: the store's refusals of a write's data (a key repeated at any
//   depth, a row or patch past `ROW_MAX_BYTES` or `ROW_MAX_COLUMNS`) are
//   `StoreError::RowRefused`, words the model may act on, never `Invalid`
//   (which stops the step); the guards themselves are unchanged.
// - R4: a key that holds its write answers only the same write again: the
//   same tool call id, actor, table and op kind. Anything else at the key
//   is refused in words, and nothing changes.
// Written before the patches.

/// R1: every refusal of the data itself is `StoreError::RowRefused`, in
/// the words the earlier tests pin (`duplicate`, `byte`, `column`), never
/// echoing the data, with nothing changed and the call left `issued`: an
/// insert repeating a nested key, an insert past either cap, a patch
/// repeating a key or past either cap, and an update whose merged row
/// passes either cap.
#[test]
fn a_refusal_of_the_data_is_row_refused() {
    let bytes = am_store::ROW_MAX_BYTES;
    let cols = am_store::ROW_MAX_COLUMNS;
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let columns = |from: usize, n: usize| {
        let cells: Vec<String> = (from..from + n).map(|i| format!(r#""c{i}":{i}"#)).collect();
        format!("{{{}}}", cells.join(","))
    };
    // A row near the byte cap, and a row at the column cap.
    let big = r.insert(
        &a,
        1,
        &json!({ "ref": "r-big", "title": "b".repeat(bytes / 2 + 100) }),
    );
    let call = r.issue(&a, 2);
    let wide = Uuid::now_v7();
    r.write_at(
        &a,
        2,
        &call,
        RowOp::Insert {
            row_id: wide,
            data_json: columns(0, cols),
        },
    )
    .unwrap();
    let big_id = Uuid::parse_str(&big).unwrap();

    let cases: Vec<(&str, RowOp, &str)> = vec![
        (
            "an insert repeating a nested key",
            RowOp::Insert {
                row_id: Uuid::now_v7(),
                data_json: r#"{"ref":"r-1","extra":{"canary_k":1,"canary_k":2}}"#.into(),
            },
            "duplicate",
        ),
        (
            "an insert past the byte cap",
            RowOp::Insert {
                row_id: Uuid::now_v7(),
                data_json: format!(r#"{{"ref":"{}"}}"#, "q".repeat(bytes)),
            },
            "byte",
        ),
        (
            "an insert past the column cap",
            RowOp::Insert {
                row_id: Uuid::now_v7(),
                data_json: columns(0, cols + 1),
            },
            "column",
        ),
        (
            "a patch repeating a key",
            RowOp::Update {
                row_id: big_id,
                expected_version: 1,
                patch_json: r#"{"canary_k":"x","canary_k":"y"}"#.into(),
            },
            "duplicate",
        ),
        (
            "a patch past the byte cap",
            RowOp::Update {
                row_id: big_id,
                expected_version: 1,
                patch_json: format!(r#"{{"title":"{}"}}"#, "q".repeat(bytes)),
            },
            "byte",
        ),
        (
            "a patch past the column cap",
            RowOp::Update {
                row_id: big_id,
                expected_version: 1,
                patch_json: columns(1000, cols + 1),
            },
            "column",
        ),
        (
            "an update whose merged row passes the byte cap",
            RowOp::Update {
                row_id: big_id,
                expected_version: 1,
                patch_json: format!(r#"{{"extra":"{}"}}"#, "m".repeat(bytes / 2)),
            },
            "byte",
        ),
        (
            "an update whose merged row passes the column cap",
            RowOp::Update {
                row_id: wide,
                expected_version: 1,
                patch_json: r#"{"one_more":1}"#.into(),
            },
            "column",
        ),
    ];
    for (n, (what, op, word)) in cases.into_iter().enumerate() {
        let seq = 3 + n as i64;
        let call = r.issue(&a, seq);
        let before = r.state();
        let refused = r.write_at(&a, seq, &call, op).expect_err(what);
        match &refused {
            StoreError::RowRefused(words) => {
                assert!(words.contains(word), "{what}: names `{word}`: {words}");
                for canary in ["canary_k", "qqqqqqqq", "mmmmmmmm"] {
                    assert!(!words.contains(canary), "{what}: echoes: {words}");
                }
            }
            other => panic!("{what}: RowRefused, not {other:?}"),
        }
        assert_eq!(r.state(), before, "{what}: nothing changed");
        assert_eq!(r.tool_call(&a, &call).status, "issued", "{what}");
    }
}

/// R4: at a key that holds its write (call 1, an insert), the same write
/// again (the same tool call id, the attempt's own seat, the same table,
/// an insert, with another row id and other data) is a replay: `Ok`,
/// nothing written. Each other write at the key is refused in words
/// (`refused`), never echoing its data, and nothing changes: another tool
/// call id, another seat of the run, the engine's own actor, a Human,
/// another table, and an update. After the attempt finished, the same
/// write is still a replay, and another tool call id is still refused.
#[test]
fn a_replay_is_the_same_write_or_refused() {
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let call = r.issue(&a, 1);
    let first = Uuid::now_v7();
    r.write_at(&a, 1, &call, insert_op(first, &json!({ "ref": "r-1" })))
        .unwrap();
    let once = r.state();
    let other_seat = Actor::Seat {
        seat: "another-seat".into(),
        run: a.run.uuid,
        step: a.key.clone(),
    };
    let replay = |actor: Actor, tool_call: &str, table: &str, op: RowOp| {
        r.apply(actor, row_write(&a, 1, tool_call, table, op, Taint::Clean))
    };
    let new_insert = || insert_op(Uuid::now_v7(), &json!({ "ref": CANARY_DATA }));

    replay(a.actor(), &call, TABLE, new_insert()).expect("the same write again is a replay");
    assert_eq!(r.state(), once, "a replay writes nothing");

    let other_call = new_id();
    for (what, actor, tool_call, table, op) in [
        (
            "another tool call id",
            a.actor(),
            other_call.as_str(),
            TABLE,
            new_insert(),
        ),
        (
            "another seat",
            other_seat,
            call.as_str(),
            TABLE,
            new_insert(),
        ),
        ("the engine", engine(), call.as_str(), TABLE, new_insert()),
        (
            "a Human",
            you_reviewer(),
            call.as_str(),
            TABLE,
            new_insert(),
        ),
        (
            "another table",
            a.actor(),
            call.as_str(),
            OTHER_TABLE,
            new_insert(),
        ),
        (
            "an update",
            a.actor(),
            call.as_str(),
            TABLE,
            update_op(first, 1, &json!({ "title": CANARY_PATCH })),
        ),
    ] {
        let words = replay(actor, tool_call, table, op)
            .expect_err(what)
            .to_string();
        assert!(words.contains("refused"), "{what}: in words: {words}");
        for canary in [CANARY_DATA, CANARY_PATCH] {
            assert!(!words.contains(canary), "{what}: echoes: {words}");
        }
        assert_eq!(r.state(), once, "{what}: nothing changed");
    }

    r.finish(&a);
    let finished = r.state();
    replay(a.actor(), &call, TABLE, new_insert())
        .expect("the same write is a replay after the attempt finished");
    replay(a.actor(), &other_call, TABLE, new_insert())
        .expect_err("another tool call id is refused after the attempt finished");
    assert_eq!(r.state(), finished);
}

// ------------------------------------- ToolGate slice, review round 2 (3.23)
//
// Review triage round 2 (manager, 2026-11-25), T3: since R1 the store's
// refusal of an update whose merged row passes `ROW_MAX_BYTES` reaches the
// model as an untrusted result, so its words must not tell the model the
// size of a stored row it cannot read. They may name the caps and the
// patch's own size. Written before the patch.

/// The decimal numbers standing as words of their own in `text` (digit
/// runs between non-alphanumeric characters), as written.
fn numbers_in(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

/// T3: a row of about half the byte cap, then an update whose patch (also
/// about half, within the cap on its own) would merge it past
/// `ROW_MAX_BYTES`. The refusal is `RowRefused` and names `byte` (R1), and
/// no number in it is derived from the stored row: not the row's size, not
/// the merged row's (within a margin), not how far it would pass the cap,
/// and not the room the row has left. The caps and the patch's own size
/// may stand in it. Nothing changes.
#[test]
fn a_merged_row_refusal_names_no_stored_size() {
    let cap = am_store::ROW_MAX_BYTES;
    let r = rig();
    let a = r.begin(&r.run, "collect", Some(TABLE), None, Taint::Clean);
    let id = r.insert(
        &a,
        1,
        &json!({ "ref": "r-big", "title": "b".repeat(cap / 2 + 333) }),
    );
    let stored = r.row(&id).data.len();
    let patch = format!(r#"{{"extra":"{}"}}"#, "m".repeat(cap / 2 - 77));
    assert!(patch.len() < cap && stored < cap && stored + patch.len() > cap);
    let call = r.issue(&a, 2);
    let before = r.state();

    let refused = r
        .write_at(
            &a,
            2,
            &call,
            RowOp::Update {
                row_id: Uuid::parse_str(&id).unwrap(),
                expected_version: 1,
                patch_json: patch.clone(),
            },
        )
        .expect_err("a merged row past the byte cap");

    let words = match &refused {
        StoreError::RowRefused(words) => words.clone(),
        other => panic!("RowRefused (R1), not {other:?}"),
    };
    assert!(words.contains("byte"), "{words}");
    let merged = stored + patch.len();
    let mut derived: Vec<usize> = vec![stored, cap - stored];
    derived.extend(merged - 16..=merged + 16);
    derived.extend(merged - 16 - cap..=merged + 16 - cap);
    let derived: Vec<String> = derived
        .into_iter()
        .filter(|n| *n != cap && *n != patch.len())
        .map(|n| n.to_string())
        .collect();
    for number in numbers_in(&words) {
        assert!(
            !derived.contains(&number),
            "`{number}` is derived from the stored row's size ({stored} bytes): {words}"
        );
    }
    assert_eq!(r.state(), before, "nothing changed");
}
