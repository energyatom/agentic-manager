//! Epic 3 story 5 at the store (AD-7, AD-8, AD-10, AD-25, AD-27, AD-35,
//! AD-36, AD-43): the journal keeps what a model was sent.
//!
//! - Migration 0009 adds `prompt_parts` (one row per part of an attempt's
//!   prompt, or of a repair call's) and `model_calls.purpose` (`turn` or
//!   `repair`). It runs behind AD-7's pre-migration copy, keeps every row
//!   of every table value for value, marks every old call a `turn` and
//!   gives no old attempt any part.
//! - `WriteCommand::StartAttempt { prompt, .. }` journals the assembled
//!   parts in the transaction that starts the attempt; the writer checks
//!   AD-27's slot order, that a slot holds only its kind, that no
//!   instruction or trusted row is tainted and no untrusted row is clean,
//!   and that an input part matches a journaled input of the attempt. An
//!   `untrusted` part raises the attempt's taint in the same transaction.
//! - `WriteCommand::ReserveCall { purpose, prompt, .. }`: a `repair` call
//!   carries its own two parts (the invalid reply as an `assistant` row,
//!   the schema's repair `instruction`), only once per attempt and only
//!   after a settled first turn (AD-36: one repair, then fail).
//! - The rows are append-only (a trigger aborts an `UPDATE` or a `DELETE`);
//!   the audit entries hold counts and the purpose, never a part's text.
//!
//! The pinned API is in the story plan's Implementation Notes, "API the
//! tests fix". Written before migration 0009 or any of it exists.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use am_domain::{Actor, RuntimeLayout, Taint};
use am_store::audit::sha256_hex;
use am_store::{
    CallPurpose, NewPromptPart, OpenReport, PromptKind, PromptSlot, Reader, SCHEMA_VERSION,
    StepInput, Store, StoreError, WriteCommand,
};
use rusqlite::types::Value as Cell;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

const HASH: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";

/// A canary no audit entry may hold.
const CANARY: &str = "canary-prompt-text-31e8a0";

/// The schema version before migration 0009.
const BEFORE_PROMPT_PARTS: i64 = 8;

fn sys() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

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

fn audit_entries(reader: &Reader) -> Vec<Value> {
    let mut out = Vec::new();
    reader.export_audit_jsonl(&mut out).unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// A taint flag as an audit argument: `1` or `true` (the writer's choice).
fn audited_tainted(arg: &Value) -> bool {
    arg == &Value::from(1) || arg == &Value::Bool(true)
}

// ------------------------------------------------------------ migration 0009

/// A schema-8 database in the project's runtime folder, made by hand.
fn database_at_8(root: &Path) -> PathBuf {
    existing_runtime(root);
    let path = db(root);
    am_store::testing::database_at_version(&path, BEFORE_PROMPT_PARTS).unwrap();
    path
}

/// Rows in every table a run touches, as 3.3's and 3.4's writers made them:
/// two runs (one running with a tainted attempt, one failed), attempts,
/// settled and open calls with their ledger rows, inputs (one from an
/// earlier attempt, one plan input), a tool call, an open Review and an
/// open Failure item, a trigger and a fire.
fn rows_at_8(path: &Path) {
    let step = "id, run_id, step_key, seq, attempt, status, output, taint, updated_at, started_at, finished_at";
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
             INSERT INTO model_calls (id, step_run_id, call_seq, status, settled, input_tokens, output_tokens, created_at)
               VALUES ('c1', 'r1-a', 1, 'settled', 1, 12, 7, '2026-11-20T10:00:02.000Z');
             INSERT INTO model_calls (id, step_run_id, call_seq, status, settled, input_tokens, output_tokens, created_at)
               VALUES ('c2', 'r1-b', 1, 'reserved', 0, NULL, NULL, '2026-11-20T10:00:05.000Z');
             INSERT INTO model_calls (id, step_run_id, call_seq, status, settled, input_tokens, output_tokens, created_at)
               VALUES ('c3', 'r2-a', 1, 'settled', 1, 3, 0, '2026-11-20T11:00:02.000Z');
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
             INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at)
               VALUES ('i2', 'failure', 'open', 'r2', 'r2-a', 'r2-a', 0, 'A step stopped', 'Step a stopped.', '2026-11-20T11:00:09.000Z');
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

/// Row 30: every row of every table the schema-8 database held is the
/// same value for value after 0009 (the two logs a migration appends to,
/// `audit_log` and `migration_reports`, keep their old rows as a prefix);
/// the pre-migration copy the report names exists, opens at version 8 and
/// holds every row as it was, also after a reopen; the step is reported
/// 8 to 9 and audited.
#[test]
fn migration_0009_keeps_every_row_and_the_pre_migration_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_8(dir.path());
    rows_at_8(&path);
    let before = tables_of(&path);
    for table in [
        "runs",
        "step_runs",
        "model_calls",
        "ledger",
        "step_inputs",
        "tool_calls",
        "inbox_items",
        "triggers",
        "fires",
    ] {
        assert!(!before[table].1.is_empty(), "{table} holds rows to keep");
    }
    assert!(!before.contains_key("prompt_parts"));

    let (store, report) = open_project(dir.path()).unwrap();

    // Story 3.6 adds 0010 after it: the 8 to 9 step is applied (and later
    // steps after it), not necessarily the last.
    const { assert!(SCHEMA_VERSION > BEFORE_PROMPT_PARTS) };
    assert_eq!(report.version, SCHEMA_VERSION);
    let step = report
        .applied
        .iter()
        .find(|m| m.from == BEFORE_PROMPT_PARTS)
        .expect("the 8 to 9 step is reported");
    assert_eq!(step.to, BEFORE_PROMPT_PARTS + 1);
    let backup = step.backup.clone().expect("a pre-migration copy (AD-7)");
    assert!(backup.is_file(), "{}", backup.display());
    assert!(
        report
            .lines()
            .iter()
            .any(|l| l.contains("from version 8 to 9")),
        "{:?}",
        report.lines()
    );

    let after = tables_of(&path);
    for (table, (cols, rows)) in &before {
        let (_, now) = after
            .get(table)
            .unwrap_or_else(|| panic!("{table} was dropped"));
        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
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

    let copy_is_as_it_was = |what: &str| {
        let copy = Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(user_version(&copy), BEFORE_PROMPT_PARTS, "{what}");
        drop(copy);
        assert_eq!(
            tables_of(&backup),
            before,
            "{what}: the copy holds every row"
        );
    };
    copy_is_as_it_was("after the upgrade");

    let reader = store.reader().unwrap();
    let all = audit_entries(&reader);
    assert!(
        all.iter().any(|e| e["action"] == "store.migrated"
            && e["outcome"]
                .as_str()
                .is_some_and(|o| o.starts_with("from version 8 to 9"))),
        "{all:#?}"
    );
    assert_eq!(reader.verify_audit_chain().unwrap(), None);
    drop(reader);
    drop(store);
    let (_store, again) = open_project(dir.path()).unwrap();
    assert!(again.applied.is_empty(), "reopening applies nothing");
    copy_is_as_it_was("after a reopen");
}

/// Row 30: `prompt_parts` and `model_calls.purpose` exist; every call the
/// old database held is a `turn`; no old attempt has a part (the reader
/// returns none), and `purpose` refuses a word but `turn` and `repair`.
#[test]
fn migration_0009_marks_old_calls_as_turns_and_leaves_no_parts() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_8(dir.path());
    rows_at_8(&path);

    let (store, _) = open_project(dir.path()).unwrap();

    assert!(!columns(&path, "prompt_parts").is_empty(), "the table");
    for column in [
        "step_run_id",
        "call_seq",
        "ord",
        "slot",
        "kind",
        "label",
        "taint",
        "text",
        "bytes",
        "sha256",
        "created_at",
    ] {
        assert!(
            columns(&path, "prompt_parts").contains(&column.to_string()),
            "prompt_parts.{column}"
        );
    }
    assert!(columns(&path, "model_calls").contains(&"purpose".to_string()));
    let c = Connection::open(&path).unwrap();
    let purposes: Vec<String> = c
        .prepare("SELECT purpose FROM model_calls ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(purposes, ["turn", "turn", "turn"]);
    assert!(
        c.execute(
            "UPDATE model_calls SET purpose = 'other' WHERE id = 'c1'",
            []
        )
        .is_err(),
        "purpose is `turn` or `repair`"
    );
    drop(c);

    let reader = store.reader().unwrap();
    for id in ["r1-a", "r1-b", "r1-c", "r2-a"] {
        assert!(reader.prompt_parts(id).unwrap().is_empty(), "{id}");
    }
    for call in reader.calls_for_run("r1").unwrap() {
        assert_eq!(call.purpose, CallPurpose::Turn, "{}", call.id);
    }
}

/// Row 30: a database already at version 9 opens with nothing applied and
/// no new pre-migration copy.
#[test]
fn reopening_a_version_9_database_applies_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_8(dir.path());
    rows_at_8(&path);
    let (store, first) = open_project(dir.path()).unwrap();
    // Story 3.6: the 8 to 9 step is among the steps applied (0010 follows).
    assert!(
        first
            .applied
            .iter()
            .any(|m| (m.from, m.to) == (BEFORE_PROMPT_PARTS, BEFORE_PROMPT_PARTS + 1)),
        "{first:?}"
    );
    drop(store);
    let before = tables_of(&path);

    let (_store, again) = open_project(dir.path()).unwrap();

    assert!(again.applied.is_empty(), "{again:?}");
    assert_eq!(again.version, SCHEMA_VERSION);
    let c = Connection::open(&path).unwrap();
    assert_eq!(user_version(&c), SCHEMA_VERSION);
    drop(c);
    let after = tables_of(&path);
    for table in before.keys().filter(|t| *t != "audit_log") {
        assert_eq!(after[table], before[table], "{table}");
    }
}

/// Row 30: the step fails closed and names its backup. A table of the
/// name `prompt_parts` made by hand stops 0009; the database is left at
/// version 8 with every row, and the error holds the copy's path.
#[test]
fn a_failing_0009_names_the_backup_and_leaves_version_8() {
    let dir = tempfile::tempdir().unwrap();
    let path = database_at_8(dir.path());
    rows_at_8(&path);
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE prompt_parts (forged TEXT);")
        .unwrap();
    let before = tables_of(&path);

    let refused = match open_project(dir.path()) {
        Ok((_, report)) => panic!("the forged table must fail 0009 closed: {report:?}"),
        Err(e) => e,
    };

    let words = refused.to_string();
    match &refused {
        StoreError::MigrationFailed {
            from, to, backup, ..
        } => {
            assert_eq!((*from, *to), (BEFORE_PROMPT_PARTS, BEFORE_PROMPT_PARTS + 1));
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
    assert_eq!(
        user_version(&c),
        BEFORE_PROMPT_PARTS,
        "the step rolled back"
    );
    drop(c);
    assert_eq!(tables_of(&path), before, "every row as it was");
}

// ------------------------------------------------------------ the writer

fn part(
    slot: PromptSlot,
    kind: PromptKind,
    label: &str,
    taint: Taint,
    text: Option<&str>,
) -> NewPromptPart {
    NewPromptPart {
        slot,
        kind,
        label: label.into(),
        taint,
        text: text.map(str::to_string),
    }
}

/// An instruction part (clean, with its text).
fn instruction(slot: PromptSlot, label: &str, text: &str) -> NewPromptPart {
    part(
        slot,
        PromptKind::Instruction,
        label,
        Taint::Clean,
        Some(text),
    )
}

/// The AD-27 prompt of a step with only the role and the step instruction.
fn minimal() -> Vec<NewPromptPart> {
    vec![
        instruction(PromptSlot::Role, "Role instructions", "role text"),
        instruction(PromptSlot::Step, "Step instruction", "step text"),
    ]
}

/// A repair call's two parts: the invalid reply (carrying the attempt's
/// taint) and the schema's repair instruction.
fn repair_parts(attempt: Taint) -> Vec<NewPromptPart> {
    vec![
        part(
            PromptSlot::Reply,
            PromptKind::Assistant,
            "reply:b@1",
            attempt,
            Some("the invalid reply"),
        ),
        instruction(PromptSlot::Repair, "repair", "answer as the schema says"),
    ]
}

/// A store with a run of steps `a`, `b` and `c`.
struct Rig {
    _dir: tempfile::TempDir,
    store: Store,
    db: PathBuf,
    a: String,
    b: String,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let (store, _) = open_project(dir.path()).unwrap();
    store
        .writer()
        .exec(
            owner(),
            WriteCommand::CreateRun {
                run_id: "run-1".into(),
                workflow: "wf".into(),
                origin: "manual".into(),
                steps: vec!["a".into(), "b".into(), "c".into()],
                config_hash: Some(HASH.into()),
            },
        )
        .unwrap();
    let ids: Vec<String> = store
        .reader()
        .unwrap()
        .steps("run-1")
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    Rig {
        db: db(dir.path()),
        _dir: dir,
        store,
        a: ids[0].clone(),
        b: ids[1].clone(),
    }
}

impl Rig {
    fn exec(&self, cmd: WriteCommand) -> Result<(), StoreError> {
        self.store.writer().exec(sys(), cmd)
    }

    fn reader(&self) -> Reader {
        self.store.reader().unwrap()
    }

    /// Step `a` runs to `succeeded` with `output` (a clean attempt).
    fn finish_a(&self, output: &str) {
        self.exec(WriteCommand::StartAttempt {
            step_run_id: self.a.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: minimal(),
            writes: am_store::AttemptWrites {
                seat: "seat-a".into(),
                table: None,
                document: None,
            },
        })
        .unwrap();
        self.reserve(&self.a, "call-a", 1, CallPurpose::Turn, vec![])
            .unwrap();
        self.exec(WriteCommand::SettleAndFinishStep {
            call_id: "call-a".into(),
            usage: None,
            step_run_id: self.a.clone(),
            status: "succeeded".into(),
            output: Some(output.into()),
        })
        .unwrap();
    }

    fn start_b(&self, inputs: Vec<StepInput>, taint: Taint, prompt: Vec<NewPromptPart>) {
        self.try_start_b(inputs, taint, prompt).unwrap();
    }

    fn try_start_b(
        &self,
        inputs: Vec<StepInput>,
        taint: Taint,
        prompt: Vec<NewPromptPart>,
    ) -> Result<(), StoreError> {
        self.exec(WriteCommand::StartAttempt {
            step_run_id: self.b.clone(),
            inputs,
            taint,
            prompt,
            writes: am_store::AttemptWrites {
                seat: "seat-a".into(),
                table: None,
                document: None,
            },
        })
    }

    fn reserve(
        &self,
        step: &str,
        call: &str,
        seq: i64,
        purpose: CallPurpose,
        prompt: Vec<NewPromptPart>,
    ) -> Result<(), StoreError> {
        self.exec(WriteCommand::ReserveCall {
            step_run_id: step.into(),
            call_id: call.into(),
            call_seq: seq,
            price: am_domain::money::CallPrice::local(),
            rate: am_domain::money::Rate::none("EUR"),
            counted_input_tokens: 0,
            max_output_tokens: 0,
            week: "2026-11-23".into(),
            team: "core".into(),
            taint: Taint::Clean,
            purpose,
            prompt,
        })
    }

    fn settle(&self, call: &str) {
        self.exec(WriteCommand::SettleCall {
            call_id: call.into(),
            usage: None,
        })
        .unwrap();
    }

    fn b_row(&self) -> am_store::StepRow {
        self.reader()
            .steps("run-1")
            .unwrap()
            .into_iter()
            .find(|s| s.id == self.b)
            .unwrap()
    }

    /// What a refused command must leave as it was.
    fn state(&self) -> (Vec<am_store::StepRow>, usize, usize, usize) {
        let r = self.reader();
        (
            r.steps("run-1").unwrap(),
            audit_entries(&r).len(),
            r.prompt_parts(&self.b).unwrap().len(),
            r.calls_for_step(&self.b).unwrap().len(),
        )
    }
}

fn step_input(kind: &str, from: Option<&str>, label: &str, taint: Taint) -> StepInput {
    StepInput {
        kind: kind.into(),
        from_step_run: from.map(str::to_string),
        from_item: None,
        from_row: None,
        label: label.into(),
        taint,
    }
}

/// Row 2: the parts of AD-27's full order, a repeat of a skill included,
/// are journaled with the start: `ord` 1..n under `call_seq` 0, each with
/// its slot, kind, label and taint; the config and plan text is kept, and
/// an input from an earlier step's output has no text (it stays on the
/// producer's row: AD-43). The attempt's taint rises with its untrusted
/// input, and the `step.started` entry counts the parts.
#[test]
fn start_attempt_journals_the_parts_in_order() {
    let r = rig();
    r.finish_a("out of a");
    let prompt = vec![
        instruction(PromptSlot::Role, "Role instructions", "role text"),
        instruction(PromptSlot::Goal, "Goal", "goal text"),
        instruction(PromptSlot::Backstory, "Backstory", "backstory text"),
        instruction(PromptSlot::Skill, "skill:brief", "skill text"),
        instruction(PromptSlot::Skill, "skill:other", "other skill text"),
        instruction(PromptSlot::CeoGoal, "CEO goal:g1", "ceo text"),
        instruction(PromptSlot::Guidance, "Guidance:n1", "guidance text"),
        instruction(PromptSlot::Memory, "Memory:m1", "memory text"),
        instruction(PromptSlot::ReferenceFiles, "Reference files", "guide.md"),
        instruction(
            PromptSlot::SavedLinks,
            "Saved links",
            "Home — https://example.org/",
        ),
        instruction(PromptSlot::Step, "Step instruction", "step text"),
        part(
            PromptSlot::Trusted,
            PromptKind::Trusted,
            "step:a@1",
            Taint::Clean,
            None,
        ),
        part(
            PromptSlot::Untrusted,
            PromptKind::Untrusted,
            "test:page",
            Taint::Tainted,
            Some("page text"),
        ),
    ];
    let inputs = vec![
        step_input("step_output", Some(&r.a), "step:a@1", Taint::Clean),
        step_input("untrusted", None, "test:page", Taint::Tainted),
    ];

    r.start_b(inputs, Taint::Clean, prompt.clone());

    let rows = r.reader().prompt_parts(&r.b).unwrap();
    assert_eq!(rows.len(), prompt.len());
    for (i, (row, want)) in rows.iter().zip(&prompt).enumerate() {
        let at = format!("part {}: {want:?}", i + 1);
        assert_eq!(row.call_seq, 0, "{at}: the attempt's own prompt");
        assert_eq!(row.ord, i as i64 + 1, "{at}");
        assert_eq!(row.slot, want.slot, "{at}");
        assert_eq!(row.kind, want.kind, "{at}");
        assert_eq!(row.label, want.label, "{at}");
        assert_eq!(row.taint, want.taint, "{at}");
        assert_eq!(row.text, want.text, "{at}");
    }
    assert_eq!(
        rows.iter().map(|p| p.kind).collect::<Vec<_>>()[..11],
        [PromptKind::Instruction; 11],
        "slots 1 to 10 are instructions"
    );
    assert_eq!(
        rows[11].text, None,
        "a step output's text stays on its producer"
    );
    assert_eq!(rows[12].taint, Taint::Tainted);
    assert_eq!(
        r.b_row().taint,
        Taint::Tainted,
        "the input tainted the attempt"
    );

    let all = audit_entries(&r.reader());
    let started = all
        .iter()
        .find(|e| e["action"] == "step.started" && e["target"] == r.b.as_str())
        .expect("step.started");
    assert_eq!(started["args"]["prompt_parts"], prompt.len(), "{started}");
}

/// Row 14, the type wall (AD-10, AD-27): the writer refuses, in words
/// that start `refused` and name the offending part, a prompt that is out
/// of AD-27's order, a repeat in a slot that does not repeat, an
/// `untrusted` part in an instruction slot, an `instruction` or `trusted`
/// part with taint set, an `untrusted` part with taint clear, and a repair
/// slot on an attempt's own prompt. Nothing is written: the step stays
/// `queued`, no part, no audit entry.
#[test]
fn the_writer_refuses_parts_out_of_order_or_of_the_wrong_kind() {
    let r = rig();
    r.finish_a("out of a");
    let page = step_input("untrusted", None, "test:page", Taint::Tainted);
    let cases: Vec<(&str, Vec<StepInput>, Vec<NewPromptPart>, &str)> = vec![
        (
            "out of order",
            vec![],
            vec![
                instruction(PromptSlot::Step, "Step instruction", "step text"),
                instruction(PromptSlot::Goal, "Goal", "goal text"),
            ],
            "Goal",
        ),
        (
            "a slot that does not repeat",
            vec![],
            vec![
                instruction(PromptSlot::Goal, "Goal", "goal text"),
                instruction(PromptSlot::Goal, "Goal", "goal again"),
            ],
            "Goal",
        ),
        (
            "an untrusted part in an instruction slot",
            vec![page.clone()],
            vec![part(
                PromptSlot::Goal,
                PromptKind::Untrusted,
                "Goal",
                Taint::Tainted,
                Some("text"),
            )],
            "Goal",
        ),
        (
            "an instruction part with taint set",
            vec![],
            vec![part(
                PromptSlot::Backstory,
                PromptKind::Instruction,
                "Backstory",
                Taint::Tainted,
                Some("text"),
            )],
            "Backstory",
        ),
        (
            "a trusted part with taint set",
            vec![step_input(
                "step_output",
                Some(&r.a),
                "step:a@1",
                Taint::Clean,
            )],
            vec![part(
                PromptSlot::Trusted,
                PromptKind::Trusted,
                "step:a@1",
                Taint::Tainted,
                None,
            )],
            "step:a@1",
        ),
        (
            "an untrusted part with taint clear",
            vec![page.clone()],
            vec![part(
                PromptSlot::Untrusted,
                PromptKind::Untrusted,
                "test:page",
                Taint::Clean,
                Some("text"),
            )],
            "test:page",
        ),
        (
            "a repair slot on an attempt's own prompt",
            vec![],
            vec![part(
                PromptSlot::Reply,
                PromptKind::Assistant,
                "reply:b@1",
                Taint::Clean,
                Some("text"),
            )],
            "reply:b@1",
        ),
    ];
    let before = r.state();
    for (what, inputs, prompt, label) in cases {
        let refused = r
            .try_start_b(inputs, Taint::Clean, prompt)
            .expect_err(what)
            .to_string();
        assert!(refused.contains("refused"), "{what}: {refused}");
        assert!(refused.contains(label), "{what}: names the part: {refused}");
        assert_eq!(r.state(), before, "{what}: nothing was written");
        assert_eq!(r.b_row().status, "queued", "{what}");
    }

    // The same attempt starts with a prompt that is right.
    r.start_b(vec![], Taint::Clean, minimal());
    assert_eq!(r.b_row().status, "running");
}

/// Row 15: an `untrusted` part raises the attempt's taint in the same
/// transaction (taint only rises), even when the command says `Clean`:
/// the attempt, its `step.started` entry and the part all say tainted.
#[test]
fn an_untrusted_part_raises_the_attempts_taint() {
    let r = rig();
    let prompt = vec![
        instruction(PromptSlot::Step, "Step instruction", "step text"),
        part(
            PromptSlot::Untrusted,
            PromptKind::Untrusted,
            "test:page",
            Taint::Tainted,
            Some("page text"),
        ),
    ];

    r.start_b(
        vec![step_input("untrusted", None, "test:page", Taint::Tainted)],
        Taint::Clean,
        prompt,
    );

    assert_eq!(r.b_row().taint, Taint::Tainted);
    let rows = r.reader().prompt_parts(&r.b).unwrap();
    assert_eq!(rows[1].taint, Taint::Tainted);
    assert_eq!(rows[0].taint, Taint::Clean, "the instruction stays clean");
    let all = audit_entries(&r.reader());
    let started = all
        .iter()
        .find(|e| e["action"] == "step.started" && e["target"] == r.b.as_str())
        .unwrap();
    assert!(audited_tainted(&started["args"]["taint"]), "{started}");
}

/// Row 9: a `trusted` or `untrusted` part names an input of the attempt:
/// its label must match a `step_inputs` row journaled for it. A label with
/// no such input is refused, nothing written; the matching one starts.
#[test]
fn an_input_part_must_match_a_journaled_input() {
    let r = rig();
    r.finish_a("out of a");
    let trusted = |label: &str| {
        vec![
            instruction(PromptSlot::Step, "Step instruction", "step text"),
            part(
                PromptSlot::Trusted,
                PromptKind::Trusted,
                label,
                Taint::Clean,
                None,
            ),
        ]
    };
    let before = r.state();

    // No input at all.
    let none = r
        .try_start_b(vec![], Taint::Clean, trusted("step:a@1"))
        .expect_err("a trusted part with no input")
        .to_string();
    assert!(
        none.contains("refused") && none.contains("step:a@1"),
        "{none}"
    );
    // An input of another label.
    let other = r
        .try_start_b(
            vec![step_input(
                "step_output",
                Some(&r.a),
                "step:a@1",
                Taint::Clean,
            )],
            Taint::Clean,
            trusted("step:z@9"),
        )
        .expect_err("a label no input holds")
        .to_string();
    assert!(
        other.contains("refused") && other.contains("step:z@9"),
        "{other}"
    );
    // An untrusted part naming no plan input.
    let page = r
        .try_start_b(
            vec![step_input("untrusted", None, "test:other", Taint::Tainted)],
            Taint::Clean,
            vec![
                instruction(PromptSlot::Step, "Step instruction", "step text"),
                part(
                    PromptSlot::Untrusted,
                    PromptKind::Untrusted,
                    "test:page",
                    Taint::Tainted,
                    Some("text"),
                ),
            ],
        )
        .expect_err("an untrusted part no input holds")
        .to_string();
    assert!(
        page.contains("refused") && page.contains("test:page"),
        "{page}"
    );
    assert_eq!(r.state(), before, "nothing was written");

    r.start_b(
        vec![step_input(
            "step_output",
            Some(&r.a),
            "step:a@1",
            Taint::Clean,
        )],
        Taint::Clean,
        trusted("step:a@1"),
    );
    assert_eq!(r.reader().prompt_parts(&r.b).unwrap().len(), 2);
}

/// Row 28 (AD-36): a repair is the second call at most. The writer refuses
/// a repair with no settled first turn: none made, or the first not
/// settled; nothing is written for the refusal (no call, no ledger row, no
/// part). Once the first turn is settled the repair reserves, and its
/// `call_seq` and parts are its own.
#[test]
fn a_repair_needs_a_settled_first_turn() {
    let r = rig();
    r.start_b(vec![], Taint::Clean, minimal());
    let parts = repair_parts(Taint::Clean);

    let none = r
        .reserve(&r.b, "call-0", 1, CallPurpose::Repair, parts.clone())
        .expect_err("a repair with no first turn")
        .to_string();
    assert!(
        none.contains("refused") && none.contains("repair"),
        "{none}"
    );
    assert!(r.reader().calls_for_step(&r.b).unwrap().is_empty());

    r.reserve(&r.b, "call-1", 1, CallPurpose::Turn, vec![])
        .unwrap();
    let before = r.state();
    let ledger = r.reader().ledger_for_run("run-1").unwrap().len();
    let unsettled = r
        .reserve(&r.b, "call-2", 2, CallPurpose::Repair, parts.clone())
        .expect_err("a repair while the first turn is unsettled")
        .to_string();
    assert!(
        unsettled.contains("refused") && unsettled.contains("repair"),
        "{unsettled}"
    );
    assert_eq!(r.state(), before, "nothing was written");
    assert_eq!(r.reader().ledger_for_run("run-1").unwrap().len(), ledger);

    r.settle("call-1");
    r.reserve(&r.b, "call-2", 2, CallPurpose::Repair, parts)
        .unwrap();
    let calls = r.reader().calls_for_step(&r.b).unwrap();
    assert_eq!(
        calls.iter().map(|c| c.purpose).collect::<Vec<_>>(),
        [CallPurpose::Turn, CallPurpose::Repair]
    );
}

/// Row 28: one repair per attempt. After a settled repair a second repair
/// reservation is refused in words; there are still two calls, two ledger
/// rows, and the parts of the attempt (`call_seq` 0) and of the one repair
/// (`call_seq` 2) only.
#[test]
fn a_second_repair_call_is_refused() {
    let r = rig();
    r.start_b(vec![], Taint::Clean, minimal());
    r.reserve(&r.b, "call-1", 1, CallPurpose::Turn, vec![])
        .unwrap();
    r.settle("call-1");
    r.reserve(
        &r.b,
        "call-2",
        2,
        CallPurpose::Repair,
        repair_parts(Taint::Clean),
    )
    .unwrap();
    r.settle("call-2");
    let before = r.state();

    let refused = r
        .reserve(
            &r.b,
            "call-3",
            3,
            CallPurpose::Repair,
            repair_parts(Taint::Clean),
        )
        .expect_err("a second repair")
        .to_string();

    assert!(
        refused.contains("refused") && refused.contains("repair"),
        "{refused}"
    );
    assert_eq!(r.state(), before, "nothing was written");
    assert_eq!(r.reader().ledger_for_run("run-1").unwrap().len(), 2);
    let seqs: Vec<i64> = r
        .reader()
        .prompt_parts(&r.b)
        .unwrap()
        .iter()
        .map(|p| p.call_seq)
        .collect();
    assert_eq!(
        seqs,
        [0, 0, 2, 2],
        "the attempt's parts, then the one repair's"
    );
}

/// Rows 17, 24 (AD-10): a repair call journals its two parts under its
/// own `call_seq`, `ord` 1 and 2: the invalid reply as an `assistant` row
/// carrying the attempt's taint (marked at rest) and the repair instruction,
/// never tainted. A repair's prompt of another shape is refused: parts on a
/// plain turn, none on a repair, an instruction first, an assistant row
/// whose taint is not the attempt's.
#[test]
fn a_repair_call_journals_its_own_parts_with_the_attempts_taint() {
    for attempt in [Taint::Clean, Taint::Tainted] {
        let r = rig();
        let inputs = match attempt {
            Taint::Clean => vec![],
            Taint::Tainted => vec![step_input("untrusted", None, "test:page", Taint::Tainted)],
        };
        let mut prompt = minimal();
        if attempt == Taint::Tainted {
            prompt.push(part(
                PromptSlot::Untrusted,
                PromptKind::Untrusted,
                "test:page",
                Taint::Tainted,
                Some("page"),
            ));
        }
        r.start_b(inputs, attempt, prompt);
        r.reserve(&r.b, "call-1", 1, CallPurpose::Turn, vec![])
            .unwrap();
        r.settle("call-1");
        let before = r.state();
        let wrong: Vec<(&str, CallPurpose, Vec<NewPromptPart>)> = vec![
            (
                "parts on a plain turn",
                CallPurpose::Turn,
                repair_parts(attempt),
            ),
            ("no parts on a repair", CallPurpose::Repair, vec![]),
            (
                "the instruction first",
                CallPurpose::Repair,
                repair_parts(attempt).into_iter().rev().collect(),
            ),
            (
                "an assistant row of another taint",
                CallPurpose::Repair,
                repair_parts(if attempt == Taint::Clean {
                    Taint::Tainted
                } else {
                    Taint::Clean
                }),
            ),
            (
                "a tainted repair instruction",
                CallPurpose::Repair,
                vec![
                    repair_parts(attempt).remove(0),
                    part(
                        PromptSlot::Repair,
                        PromptKind::Instruction,
                        "repair",
                        Taint::Tainted,
                        Some("text"),
                    ),
                ],
            ),
        ];
        for (what, purpose, parts) in wrong {
            let refused = r
                .reserve(&r.b, "call-2", 2, purpose, parts)
                .expect_err(what)
                .to_string();
            assert!(refused.contains("refused"), "{what}: {refused}");
            assert_eq!(r.state(), before, "{what}: nothing was written");
        }

        r.reserve(
            &r.b,
            "call-2",
            2,
            CallPurpose::Repair,
            repair_parts(attempt),
        )
        .unwrap();

        let rows = r.reader().prompt_parts(&r.b).unwrap();
        let repair: Vec<_> = rows.iter().filter(|p| p.call_seq == 2).collect();
        assert_eq!(repair.len(), 2, "{rows:#?}");
        assert_eq!((repair[0].ord, repair[1].ord), (1, 2));
        assert_eq!(
            (repair[0].slot, repair[0].kind, repair[0].label.as_str()),
            (PromptSlot::Reply, PromptKind::Assistant, "reply:b@1")
        );
        assert_eq!(
            repair[0].taint, attempt,
            "marked at rest with the attempt's"
        );
        assert_eq!(repair[0].text.as_deref(), Some("the invalid reply"));
        assert_eq!(
            (repair[1].slot, repair[1].kind, repair[1].label.as_str()),
            (PromptSlot::Repair, PromptKind::Instruction, "repair")
        );
        assert_eq!(
            repair[1].taint,
            Taint::Clean,
            "an instruction is never tainted"
        );
        assert_eq!(
            r.b_row().taint,
            attempt,
            "a repair neither adds nor clears taint"
        );
        assert!(
            rows.iter()
                .filter(|p| p.call_seq != 2)
                .all(|p| p.call_seq == 0),
            "{rows:#?}"
        );
    }
}

/// Row 31: the journal is append-only. A raw `UPDATE` or `DELETE` of a
/// part is aborted by a trigger, and the rows stay as written.
#[test]
fn prompt_parts_cannot_be_updated_or_deleted() {
    let r = rig();
    r.start_b(vec![], Taint::Clean, minimal());
    let before = r.reader().prompt_parts(&r.b).unwrap();
    assert_eq!(before.len(), 2);

    let c = Connection::open(&r.db).unwrap();
    let update = c
        .execute("UPDATE prompt_parts SET label = 'forged'", [])
        .expect_err("an UPDATE is aborted");
    assert!(update.to_string().contains("prompt_parts"), "{update}");
    let update_text = c
        .execute("UPDATE prompt_parts SET text = 'forged'", [])
        .expect_err("an UPDATE of the text is aborted");
    assert!(
        update_text.to_string().contains("prompt_parts"),
        "{update_text}"
    );
    let delete = c
        .execute("DELETE FROM prompt_parts", [])
        .expect_err("a DELETE is aborted");
    assert!(delete.to_string().contains("prompt_parts"), "{delete}");
    drop(c);

    assert_eq!(r.reader().prompt_parts(&r.b).unwrap(), before);
}

/// Row 32 and the owner's audit-hash decision (2026-11-24): `step.started`
/// holds the part count and `model_call.reserved` the purpose; no entry
/// holds any part's text (a canary in every part, the invalid reply and
/// the output is in none of them). Across a repaired attempt the only
/// hash argument is the existing `output_sha256`, and each value is
/// `audit::sha256_hex` of text the journal stores: no new `*_sha256` key
/// appears, and no hash of a prompt or reply is audited.
#[test]
fn audit_args_hold_counts_and_purpose_never_text() {
    let r = rig();
    let prompt = vec![
        instruction(
            PromptSlot::Role,
            "Role instructions",
            &format!("role {CANARY}"),
        ),
        instruction(PromptSlot::Goal, "Goal", &format!("goal {CANARY}")),
        instruction(
            PromptSlot::Step,
            "Step instruction",
            &format!("step {CANARY}"),
        ),
        part(
            PromptSlot::Untrusted,
            PromptKind::Untrusted,
            "test:page",
            Taint::Tainted,
            Some(&format!("page {CANARY}")),
        ),
    ];
    r.start_b(
        vec![step_input("untrusted", None, "test:page", Taint::Tainted)],
        Taint::Tainted,
        prompt,
    );
    r.reserve(&r.b, "call-1", 1, CallPurpose::Turn, vec![])
        .unwrap();
    r.settle("call-1");
    let invalid = format!("not valid {CANARY}");
    let mut repair = repair_parts(Taint::Tainted);
    repair[0].text = Some(invalid);
    repair[1].text = Some(format!("repair {CANARY}"));
    r.reserve(&r.b, "call-2", 2, CallPurpose::Repair, repair)
        .unwrap();
    let output = format!("the repaired output {CANARY}-out");
    r.exec(WriteCommand::SettleAndFinishStep {
        call_id: "call-2".into(),
        usage: None,
        step_run_id: r.b.clone(),
        status: "succeeded".into(),
        output: Some(output.clone()),
    })
    .unwrap();

    let reader = r.reader();
    let all = audit_entries(&reader);
    let started = all
        .iter()
        .find(|e| e["action"] == "step.started" && e["target"] == r.b.as_str())
        .expect("step.started");
    assert_eq!(started["args"]["prompt_parts"], 4, "{started}");
    let reserved: Vec<&Value> = all
        .iter()
        .filter(|e| e["action"] == "model_call.reserved")
        .collect();
    assert_eq!(reserved.len(), 2, "{all:#?}");
    assert_eq!(reserved[0]["args"]["purpose"], "turn", "{}", reserved[0]);
    assert_eq!(reserved[1]["args"]["purpose"], "repair", "{}", reserved[1]);

    let export = all
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!export.contains(CANARY), "no part's text in the audit log");
    assert!(!export.contains("the repaired output"), "{export}");

    // Hash arguments: only `output_sha256`, each the helper's hash of text
    // the journal stores (the step's output, or a stored part's text).
    let mut stored: Vec<String> = reader
        .prompt_parts(&r.b)
        .unwrap()
        .into_iter()
        .filter_map(|p| p.text)
        .collect();
    stored.push(output);
    let mut seen = Vec::new();
    for entry in &all {
        let Some(args) = entry["args"].as_object() else {
            continue;
        };
        for (key, value) in args.iter().filter(|(k, _)| k.ends_with("_sha256")) {
            let Some(hash) = value.as_str() else { continue };
            assert_eq!(key, "output_sha256", "no new hash key: {entry}");
            assert!(
                stored.iter().any(|t| sha256_hex(t) == hash),
                "{key} of {entry} is the helper's hash of stored text"
            );
            seen.push(hash.to_string());
        }
    }
    assert!(!seen.is_empty(), "the finish hashes the output");
}

// ------------------------------------------------------- review round 1
//
// Story 3.5 review triage round 1 (manager, 2026-11-24): R2 (the writer
// compares an input part with its journaled input on kind and taint, keeps
// `(slot, label)` unique, wants exactly one Step part in a prompt and no
// empty label) and R5 (`INSERT OR REPLACE` cannot replace a part). Written
// before the patches.

/// The Step part of [`minimal`].
fn step_part() -> NewPromptPart {
    instruction(PromptSlot::Step, "Step instruction", "step text")
}

/// R2: an input part must match its `step_inputs` row on kind and taint,
/// not only on label: a trusted part named after a plan input (kind
/// `untrusted`, tainted), an untrusted part named after a clean producer's
/// output (kind `step_output`, clean) and a trusted part named after a
/// tainted producer's output (a `step_output` input with taint set) are
/// refused, nothing written.
#[test]
fn the_writer_refuses_an_input_part_of_the_wrong_kind_or_taint() {
    let r = rig();
    r.finish_a("out of a");
    let trusted = |label: &str| {
        part(
            PromptSlot::Trusted,
            PromptKind::Trusted,
            label,
            Taint::Clean,
            None,
        )
    };
    let untrusted = |label: &str| {
        part(
            PromptSlot::Untrusted,
            PromptKind::Untrusted,
            label,
            Taint::Tainted,
            None,
        )
    };
    let cases: Vec<(&str, Vec<StepInput>, NewPromptPart, &str)> = vec![
        (
            "a trusted part naming a plan input",
            vec![step_input("untrusted", None, "test:page", Taint::Tainted)],
            trusted("test:page"),
            "test:page",
        ),
        (
            "an untrusted part naming a clean producer's output",
            vec![step_input(
                "step_output",
                Some(&r.a),
                "step:a@1",
                Taint::Clean,
            )],
            untrusted("step:a@1"),
            "step:a@1",
        ),
        (
            "a trusted part naming a tainted input of a producer",
            vec![step_input(
                "step_output",
                Some(&r.a),
                "step:a@1",
                Taint::Tainted,
            )],
            trusted("step:a@1"),
            "step:a@1",
        ),
    ];
    let before = r.state();
    for (what, inputs, input_part, label) in cases {
        let refused = r
            .try_start_b(inputs, Taint::Clean, vec![step_part(), input_part])
            .expect_err(what)
            .to_string();
        assert!(refused.contains("refused"), "{what}: {refused}");
        assert!(refused.contains(label), "{what}: names the part: {refused}");
        assert_eq!(r.state(), before, "{what}: nothing was written");
    }
}

/// R2: `(slot, label)` is unique in a prompt: two skills of one id, two
/// trusted parts of one label, two reference-file parts are refused, while
/// two skills of two ids are fine.
#[test]
fn the_writer_refuses_a_duplicate_slot_and_label() {
    let r = rig();
    r.finish_a("out of a");
    let skill = |id: &str| instruction(PromptSlot::Skill, &format!("skill:{id}"), "body");
    let input = step_input("step_output", Some(&r.a), "step:a@1", Taint::Clean);
    let trusted = part(
        PromptSlot::Trusted,
        PromptKind::Trusted,
        "step:a@1",
        Taint::Clean,
        None,
    );
    let before = r.state();
    for (what, inputs, prompt, label) in [
        (
            "two skills of one id",
            vec![],
            vec![skill("brief"), skill("brief"), step_part()],
            "skill:brief",
        ),
        (
            "two trusted parts of one label",
            vec![input.clone()],
            vec![step_part(), trusted.clone(), trusted.clone()],
            "step:a@1",
        ),
    ] {
        let refused = r
            .try_start_b(inputs, Taint::Clean, prompt)
            .expect_err(what)
            .to_string();
        assert!(refused.contains("refused"), "{what}: {refused}");
        assert!(refused.contains(label), "{what}: {refused}");
        assert_eq!(r.state(), before, "{what}: nothing was written");
    }

    r.start_b(
        vec![],
        Taint::Clean,
        vec![skill("brief"), skill("other"), step_part()],
    );
    assert_eq!(r.reader().prompt_parts(&r.b).unwrap().len(), 3);
}

/// R2: a non-empty prompt has exactly one `Step` part: none, or two, is
/// refused in words naming it; an empty prompt (a start with nothing
/// journaled, as before 3.5) is still accepted.
#[test]
fn the_writer_refuses_a_prompt_without_exactly_one_step_part() {
    let r = rig();
    let role = instruction(PromptSlot::Role, "Role instructions", "role text");
    let before = r.state();
    for (what, prompt) in [
        ("no Step part", vec![role.clone()]),
        (
            "two Step parts",
            vec![role.clone(), step_part(), step_part()],
        ),
    ] {
        let refused = r
            .try_start_b(vec![], Taint::Clean, prompt)
            .expect_err(what)
            .to_string();
        assert!(refused.contains("refused"), "{what}: {refused}");
        assert!(
            refused.to_ascii_lowercase().contains("step"),
            "{what}: names the Step part: {refused}"
        );
        assert_eq!(r.state(), before, "{what}: nothing was written");
    }

    r.start_b(vec![], Taint::Clean, vec![]);
    assert_eq!(r.b_row().status, "running");
    assert!(r.reader().prompt_parts(&r.b).unwrap().is_empty());
}

/// R2: a part has a label. An empty one (or one of spaces only) is refused,
/// in words, in an attempt's prompt and in a repair call's.
#[test]
fn the_writer_refuses_an_empty_label() {
    let r = rig();
    let before = r.state();
    for label in ["", "   "] {
        let refused = r
            .try_start_b(
                vec![],
                Taint::Clean,
                vec![
                    instruction(PromptSlot::Goal, label, "goal text"),
                    step_part(),
                ],
            )
            .expect_err(label)
            .to_string();
        assert!(
            refused.contains("refused") && refused.contains("label"),
            "{label:?}: {refused}"
        );
        assert_eq!(r.state(), before);
    }

    r.start_b(vec![], Taint::Clean, minimal());
    r.reserve(&r.b, "call-1", 1, CallPurpose::Turn, vec![])
        .unwrap();
    r.settle("call-1");
    let mut parts = repair_parts(Taint::Clean);
    parts[0].label = String::new();
    let before = r.state();
    let refused = r
        .reserve(&r.b, "call-2", 2, CallPurpose::Repair, parts)
        .expect_err("an empty label on a repair part")
        .to_string();
    assert!(
        refused.contains("refused") && refused.contains("label"),
        "{refused}"
    );
    assert_eq!(r.state(), before);
}

/// R5: a replace is a delete SQLite does not run the `BEFORE DELETE`
/// trigger for, so `INSERT OR REPLACE` and `REPLACE INTO` over an existing
/// `(step_run_id, call_seq, ord)` abort, and the row is as it was.
#[test]
fn insert_or_replace_cannot_replace_a_part() {
    let r = rig();
    r.start_b(vec![], Taint::Clean, minimal());
    let before = r.reader().prompt_parts(&r.b).unwrap();
    assert_eq!(before.len(), 2);

    let c = Connection::open(&r.db).unwrap();
    for verb in ["INSERT OR REPLACE INTO", "REPLACE INTO"] {
        let refused = c
            .execute(
                &format!(
                    "{verb} prompt_parts (step_run_id, call_seq, ord, slot, kind, label, taint, text, bytes, sha256, created_at)
                     VALUES (?1, 0, 1, 'role', 'instruction', 'forged', 0, 'forged', 6, NULL, '2026-11-24T10:00:00.000Z')"
                ),
                [&r.b],
            )
            .expect_err(verb);
        assert!(
            refused.to_string().contains("prompt_parts"),
            "{verb}: {refused}"
        );
    }
    drop(c);

    assert_eq!(r.reader().prompt_parts(&r.b).unwrap(), before);
}

// ------------------------------------------------------- review round 2

/// T2: a replace by `id` (an existing id with a new `ord`) is a replace too:
/// it aborts and the row is as it was.
#[test]
fn insert_or_replace_by_id_cannot_replace_a_part() {
    let r = rig();
    r.start_b(vec![], Taint::Clean, minimal());
    let before = r.reader().prompt_parts(&r.b).unwrap();
    let c = Connection::open(&r.db).unwrap();
    let id: i64 = c
        .query_row(
            "SELECT id FROM prompt_parts WHERE step_run_id = ?1 ORDER BY id LIMIT 1",
            [&r.b],
            |row| row.get(0),
        )
        .unwrap();
    for verb in ["INSERT OR REPLACE INTO", "REPLACE INTO"] {
        let refused = c
            .execute(
                &format!(
                    "{verb} prompt_parts (id, step_run_id, call_seq, ord, slot, kind, label, taint, text, bytes, sha256, created_at)
                     VALUES (?1, ?2, 0, 99, 'role', 'instruction', 'forged', 0, 'forged', 6, NULL, '2026-11-24T10:00:00.000Z')"
                ),
                rusqlite::params![id, r.b],
            )
            .expect_err(verb);
        assert!(
            refused.to_string().contains("prompt_parts"),
            "{verb}: {refused}"
        );
    }
    drop(c);

    assert_eq!(r.reader().prompt_parts(&r.b).unwrap(), before);
}

/// T6: the labels of a non-empty prompt's input parts are the labels of the
/// attempt's journaled inputs, both ways: an input the command journals
/// (a clean producer's output, a plan input) with no part for it is
/// refused, in words naming its label, and nothing is written.
#[test]
fn an_input_with_no_prompt_part_is_refused() {
    let r = rig();
    r.finish_a("out of a");
    let before = r.state();
    for (what, inputs, label) in [
        (
            "a producer's output with no part",
            vec![step_input(
                "step_output",
                Some(&r.a),
                "step:a@1",
                Taint::Clean,
            )],
            "step:a@1",
        ),
        (
            "a plan input with no part",
            vec![step_input("untrusted", None, "test:page", Taint::Tainted)],
            "test:page",
        ),
    ] {
        let refused = r
            .try_start_b(inputs, Taint::Clean, minimal())
            .expect_err(what)
            .to_string();
        assert!(refused.contains("refused"), "{what}: {refused}");
        assert!(
            refused.contains(label),
            "{what}: names the input: {refused}"
        );
        assert_eq!(r.state(), before, "{what}: nothing was written");
    }

    // With its part, it starts.
    let mut prompt = minimal();
    prompt.push(part(
        PromptSlot::Trusted,
        PromptKind::Trusted,
        "step:a@1",
        Taint::Clean,
        None,
    ));
    r.start_b(
        vec![step_input(
            "step_output",
            Some(&r.a),
            "step:a@1",
            Taint::Clean,
        )],
        Taint::Clean,
        prompt,
    );
    assert_eq!(r.b_row().status, "running");
}
