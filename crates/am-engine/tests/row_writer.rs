//! Epic 3 story 6 (AD-8, AD-21; plan matrix rows 2, 4 and 15): am-engine's
//! implementation of the row-writer port, `StoreRowWriter`, over the
//! store's row command (`Writer::apply_row_write`). It is the only shipped
//! file besides am-store that names the command (`cargo xtask check-deps`);
//! ToolGate is its only caller.
//!
//! - A write commits and the port answers with the row id and version the
//!   store's data-free receipt holds (`Reader::row_receipt`), never with
//!   what it was asked: an insert is version 1 `Inserted`, an update
//!   version n+1 `Updated`.
//! - The same (step run, call sequence) again writes nothing and answers
//!   the first write's row id and version. Review round 1 (R4, manager
//!   2026-11-25): with the first write's status word (`Inserted` here), so
//!   the result the model receives equals the one a resume rebuilds; the
//!   tests named `AlreadyWritten` before.
//! - The store's refusal a model may read (a stale version, an unknown row)
//!   is `RowWriteError::Refused` in words, and nothing changes.
//!
//! On a real store in a temporary folder; no network, no model. Written
//! before the port or the command exists.
#![forbid(unsafe_code)]

use std::path::PathBuf;

use am_domain::rows::{RowOp, RowStatus, RowWrite, RowWriteError, RowWriter, RowWritten};
use am_domain::{Actor, Taint};
use am_engine::row_writer::StoreRowWriter;
use am_store::{AttemptWrites, Reader, Store, WriteCommand, new_id};
use serde_json::{Value, json};
use uuid::Uuid;

const TABLE: &str = "entries";
const SEAT: &str = "seat-collect";
const STEP: &str = "collect";

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

/// A store with one run whose step `collect` is running, journaled to
/// write `entries` as `seat-collect`.
struct Rig {
    _dir: tempfile::TempDir,
    db: PathBuf,
    store: Store,
    run: Uuid,
    step_run: String,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = Store::open(&db).unwrap();
    let run = new_id();
    let w = store.writer();
    w.exec(
        engine(),
        WriteCommand::CreateRun {
            run_id: run.clone(),
            workflow: "wf".into(),
            origin: "manual".into(),
            steps: vec![STEP.into()],
            config_hash: None,
        },
    )
    .unwrap();
    let step_run = store.reader().unwrap().steps(&run).unwrap()[0].id.clone();
    w.exec(
        engine(),
        WriteCommand::StartAttempt {
            step_run_id: step_run.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
            writes: AttemptWrites {
                seat: SEAT.into(),
                table: Some(TABLE.into()),
                document: None,
            },
        },
    )
    .unwrap();
    Rig {
        _dir: dir,
        db,
        store,
        run: Uuid::parse_str(&run).unwrap(),
        step_run,
    }
}

impl Rig {
    fn seat(&self) -> Actor {
        Actor::Seat {
            seat: SEAT.into(),
            run: self.run,
            step: STEP.into(),
        }
    }

    /// A `table_write` call journaled `issued` at `call_seq`; its id.
    fn issued(&self, call_seq: i64) -> String {
        let id = new_id();
        self.store
            .writer()
            .exec(
                engine(),
                WriteCommand::RecordToolCall {
                    tool_call_id: id.clone(),
                    step_run_id: self.step_run.clone(),
                    call_seq,
                    tool: "table_write".into(),
                    status: "issued".into(),
                },
            )
            .unwrap();
        id
    }

    fn write(&self, tool_call_id: &str, call_seq: i64, op: RowOp) -> RowWrite {
        RowWrite {
            tool_call_id: tool_call_id.into(),
            step_run_id: self.step_run.clone(),
            call_seq,
            table_id: TABLE.into(),
            op,
            taint: Taint::Clean,
        }
    }

    fn reader(&self) -> Reader {
        Reader::open(&self.db).unwrap()
    }
}

fn data(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// Rows 2 and 15: the port's answers are the receipt's. An insert is
/// version 1 `Inserted` with the row id it was given; `Reader::row_receipt`
/// at its key holds the same. The same key again (the same call, seat,
/// table and op kind) writes nothing and answers the first write, with
/// its status word (`Inserted`; review round 1, R4: it was
/// `AlreadyWritten`), even with other data and another row id. An update
/// at version 1 is version 2 `Updated`.
#[test]
fn the_store_row_writer_reads_its_result_from_the_receipt() {
    let r = rig();
    let port = StoreRowWriter::new(&r.store);
    let first_id = Uuid::now_v7();
    let call_1 = r.issued(1);

    let first = port.write_row(
        r.seat(),
        r.write(
            &call_1,
            1,
            RowOp::Insert {
                row_id: first_id,
                data_json: json!({"ref": "r-1", "title": "first"}).to_string(),
            },
        ),
    );

    assert_eq!(
        first,
        Ok(RowWritten {
            row_id: first_id,
            version: 1,
            status: RowStatus::Inserted,
        })
    );
    let receipt = r
        .reader()
        .row_receipt(&r.step_run, 1)
        .unwrap()
        .expect("the receipt at the key");
    assert_eq!(
        (receipt.row_id.as_str(), receipt.version),
        (first_id.to_string().as_str(), 1)
    );
    assert_eq!(receipt.tool_call_id, call_1);
    let tools = r.reader().tool_calls(&r.step_run).unwrap();
    assert_eq!(tools[0].status, "succeeded", "the write finished its call");

    let again = port.write_row(
        r.seat(),
        r.write(
            &call_1,
            1,
            RowOp::Insert {
                row_id: Uuid::now_v7(),
                data_json: json!({"ref": "r-2"}).to_string(),
            },
        ),
    );

    assert_eq!(
        again,
        Ok(RowWritten {
            row_id: first_id,
            version: 1,
            status: RowStatus::Inserted,
        }),
        "a replay answers the first write, with its status word (R4)"
    );
    let rows = r.reader().table_rows_of(TABLE).unwrap();
    assert_eq!(rows.len(), 1, "{rows:#?}");
    assert_eq!(data(&rows[0].data), json!({"ref": "r-1", "title": "first"}));
    assert_eq!(
        r.reader()
            .row_history_of(TABLE, &first_id.to_string())
            .unwrap()
            .len(),
        1
    );

    let call_2 = r.issued(2);
    let updated = port.write_row(
        r.seat(),
        r.write(
            &call_2,
            2,
            RowOp::Update {
                row_id: first_id,
                expected_version: 1,
                patch_json: json!({"title": "second"}).to_string(),
            },
        ),
    );

    assert_eq!(
        updated,
        Ok(RowWritten {
            row_id: first_id,
            version: 2,
            status: RowStatus::Updated,
        })
    );
    let row = r
        .reader()
        .table_row_by_id(TABLE, &first_id.to_string())
        .unwrap()
        .expect("the row");
    assert_eq!(row.version, 2);
    assert_eq!(data(&row.data), json!({"ref": "r-1", "title": "second"}));
}

/// Row 4 (owner Q2): an update that names a version the row is not at is
/// `RowWriteError::Refused`, in words that name the row's current version;
/// so is an update of a row the table does not hold (its id not echoed).
/// Nothing changes: the row, its history and the calls stay `issued`.
#[test]
fn a_stale_version_maps_to_refused() {
    let r = rig();
    let port = StoreRowWriter::new(&r.store);
    let row_id = Uuid::now_v7();
    let call_1 = r.issued(1);
    port.write_row(
        r.seat(),
        r.write(
            &call_1,
            1,
            RowOp::Insert {
                row_id,
                data_json: json!({"ref": "r-1"}).to_string(),
            },
        ),
    )
    .unwrap();
    let before = (
        r.reader()
            .table_row_by_id(TABLE, &row_id.to_string())
            .unwrap(),
        r.reader()
            .row_history_of(TABLE, &row_id.to_string())
            .unwrap(),
    );

    let call_2 = r.issued(2);
    let stale = port.write_row(
        r.seat(),
        r.write(
            &call_2,
            2,
            RowOp::Update {
                row_id,
                expected_version: 2,
                patch_json: json!({"title": "late"}).to_string(),
            },
        ),
    );

    match &stale {
        Err(RowWriteError::Refused(words)) => {
            assert!(words.contains("version 1"), "{words}");
            assert!(words.contains(&row_id.to_string()), "{words}");
        }
        other => panic!("refused in words: {other:?}"),
    }

    let unknown = Uuid::now_v7();
    let call_3 = r.issued(3);
    let missing = port.write_row(
        r.seat(),
        r.write(
            &call_3,
            3,
            RowOp::Update {
                row_id: unknown,
                expected_version: 1,
                patch_json: json!({"title": "x"}).to_string(),
            },
        ),
    );

    match &missing {
        Err(RowWriteError::Refused(words)) => {
            assert!(words.contains("does not hold"), "{words}");
            assert!(!words.contains(&unknown.to_string()), "{words}");
        }
        other => panic!("refused in words: {other:?}"),
    }
    assert_eq!(
        (
            r.reader()
                .table_row_by_id(TABLE, &row_id.to_string())
                .unwrap(),
            r.reader()
                .row_history_of(TABLE, &row_id.to_string())
                .unwrap(),
        ),
        before,
        "nothing changed"
    );
    let statuses: Vec<String> = r
        .reader()
        .tool_calls(&r.step_run)
        .unwrap()
        .into_iter()
        .map(|t| t.status)
        .collect();
    assert_eq!(statuses, ["succeeded", "issued", "issued"]);
    assert_eq!(r.reader().table_rows_of(TABLE).unwrap().len(), 1);
}

/// Review round 1, R1 (manager, 2026-11-25): the store's refusals of the
/// data a model wrote come back as `RowWriteError::Refused` in the store's
/// words (an untrusted error result the step goes on after), never
/// `Failed` (which stops the step): an insert repeating a key inside a
/// nested object (`duplicate`), an update whose merged row passes the
/// byte cap (`byte`), and one whose merged row passes the column cap
/// (`column`). Nothing is written, and each call stays `issued`.
#[test]
fn a_refusal_of_the_data_maps_to_refused() {
    let r = rig();
    let port = StoreRowWriter::new(&r.store);
    let big = Uuid::now_v7();
    let call_1 = r.issued(1);
    port.write_row(
        r.seat(),
        r.write(
            &call_1,
            1,
            RowOp::Insert {
                row_id: big,
                data_json:
                    json!({"ref": "big", "title": "b".repeat(am_store::ROW_MAX_BYTES / 2 + 100)})
                        .to_string(),
            },
        ),
    )
    .unwrap();
    let wide = Uuid::now_v7();
    let call_2 = r.issued(2);
    let cells: Vec<String> = (0..am_store::ROW_MAX_COLUMNS)
        .map(|i| format!(r#""c{i}":{i}"#))
        .collect();
    port.write_row(
        r.seat(),
        r.write(
            &call_2,
            2,
            RowOp::Insert {
                row_id: wide,
                data_json: format!("{{{}}}", cells.join(",")),
            },
        ),
    )
    .unwrap();
    let rows_before = r.reader().table_rows_of(TABLE).unwrap();

    for (seq, what, op, word) in [
        (
            3,
            "a repeated nested key",
            RowOp::Insert {
                row_id: Uuid::now_v7(),
                data_json: r#"{"ref":"r-1","extra":{"dup_k":1,"dup_k":2}}"#.into(),
            },
            "duplicate",
        ),
        (
            4,
            "a merged row past the byte cap",
            RowOp::Update {
                row_id: big,
                expected_version: 1,
                patch_json: json!({"extra": "m".repeat(am_store::ROW_MAX_BYTES / 2)}).to_string(),
            },
            "byte",
        ),
        (
            5,
            "a merged row past the column cap",
            RowOp::Update {
                row_id: wide,
                expected_version: 1,
                patch_json: r#"{"one_more":1}"#.into(),
            },
            "column",
        ),
    ] {
        let call = r.issued(seq);
        match port.write_row(r.seat(), r.write(&call, seq, op)) {
            Err(RowWriteError::Refused(words)) => {
                assert!(words.contains(word), "{what}: names `{word}`: {words}")
            }
            other => panic!("{what}: refused in words the model reads: {other:?}"),
        }
        let status = r
            .reader()
            .tool_calls(&r.step_run)
            .unwrap()
            .into_iter()
            .find(|t| t.id == call)
            .unwrap()
            .status;
        assert_eq!(status, "issued", "{what}");
    }
    assert_eq!(r.reader().table_rows_of(TABLE).unwrap(), rows_before);
}
