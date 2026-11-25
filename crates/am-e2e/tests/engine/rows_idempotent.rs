//! Engine test (AD-8, AD-21, AD-43; epic 3 story 6, plan matrix rows 15, 16
//! and 19, and the ticket's verify "a repeated call with the same call
//! sequence writes once"): a row write's idempotency key is the attempt's
//! step run plus the call sequence of its journaled `table_write` call.
//!
//! - The port's `write_row` sent again with a key that already wrote
//!   writes nothing (no row, version, after-image or audit entry) and
//!   answers the first write's row id, version and status word (review
//!   round 1, R4: `Inserted`, the word a resume rebuilds; it was
//!   `AlreadyWritten`); `Reader::row_written_at` at the key gives the same
//!   after-image.
//! - A loop-back's attempt 2 starts its call sequences at 1 again and
//!   writes again (the key is the attempt, not the step key).
//! - Over every tool call of a run: a `succeeded` `table_write` call always
//!   has its after-image at its key, naming it; a `failed` or `refused` one
//!   never has one (the after-image, the row and the call's finish commit
//!   together).
//!
//! No network, no real model. Written before any of it exists.
#![forbid(unsafe_code)]

mod support;

use am_domain::rows::{RowOp, RowStatus, RowWrite, RowWriter, RowWritten};
use am_domain::{Actor, EngineResult, Taint};
use am_engine::row_writer::StoreRowWriter;
use am_engine::run::{RunOutcome, RunPlan, run_plan};
use am_engine::toolgate::engine_result_of;
use am_store::Reader;
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

async fn run(p: &Project, plan: &RunPlan) -> RunOutcome {
    run_plan(&p.store, owner(), "manual", plan, CancellationToken::new())
        .await
        .unwrap()
}

/// Row 15, the verify: a step inserts one row. The port, asked again with
/// that call's key (its step run and call sequence) as the same seat, with
/// other data and another row id, writes nothing and answers the first
/// write: its row id, version 1 and its word, `Inserted` (R4), so the
/// engine result built from that answer equals the one
/// `toolgate::engine_result_of` rebuilds for the call. The table still holds
/// the one row with its first data, one after-image and one `row.written`
/// entry; `Reader::row_written_at` at the key gives the same after-image as
/// before.
#[tokio::test]
async fn a_repeated_call_sequence_writes_once_and_reads_back_the_same_result() {
    let p = project();
    let model = Live::new(
        &p.db,
        vec![write(insert(json!({"ref": "r-1"}))), say("recorded")],
    );
    let outcome = run(&p, &plan(vec![writing(step("collect", model))])).await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);
    let call = j.tool_calls_of(&attempt.id).remove(0);
    let reader = Reader::open(&p.db).unwrap();
    let first = row_written_by(&reader, &attempt.id, call.call_seq);
    drop(reader);
    let seat = Actor::Seat {
        seat: "seat-collect".into(),
        run: uuid::Uuid::parse_str(&outcome.run_id).unwrap(),
        step: "collect".into(),
    };

    let again = StoreRowWriter::new(&p.store).write_row(
        seat,
        RowWrite {
            tool_call_id: call.id.clone(),
            step_run_id: attempt.id.clone(),
            call_seq: call.call_seq,
            table_id: TABLE.into(),
            op: RowOp::Insert {
                row_id: uuid::Uuid::now_v7(),
                data_json: json!({"ref": "r-2"}).to_string(),
            },
            taint: Taint::Clean,
        },
    );

    assert_eq!(
        again,
        Ok(RowWritten {
            row_id: uuid::Uuid::parse_str(&first.row_id).unwrap(),
            version: 1,
            status: RowStatus::Inserted,
        })
    );
    let reader = Reader::open(&p.db).unwrap();
    assert_eq!(
        engine_result_of(&reader, &attempt.id, call.call_seq).unwrap(),
        Some(EngineResult::row_written(&again.clone().unwrap())),
        "the replay's result equals the rebuilt one (R4)"
    );
    drop(reader);
    let row = only_row(&p.db, TABLE);
    assert_eq!(row.row_id, first.row_id);
    assert_eq!(data_of(&row.data), json!({"ref": "r-1"}));
    assert_eq!(history_of(&p.db, TABLE, &row.row_id).len(), 1);
    let reader = Reader::open(&p.db).unwrap();
    assert_eq!(row_written_by(&reader, &attempt.id, call.call_seq), first);
    assert_eq!(journal(&p.db).entries("row.written").len(), 1);
}

/// Row 16: `collect` writes one row per attempt; `check` loops back to it
/// once. Attempt 2's write has call sequence 1 again and is a new write: two
/// rows, two after-images, each naming its own attempt with call sequence
/// 1.
#[tokio::test]
async fn a_loop_back_attempt_writes_again() {
    let p = project();
    let collect = Live::new(
        &p.db,
        vec![
            write(insert(json!({"ref": "first-attempt"}))),
            say("one"),
            write(insert(json!({"ref": "second-attempt"}))),
            say("two"),
        ],
    );
    let check = texts(&[r#"{"reasons":["record again"]}"#, r#"{"reasons":[]}"#]);
    let plan = plan(vec![
        writing(step("collect", collect)),
        looping("check", check, "collect", 1),
    ]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let (one, two) = (j.attempt("collect", 1), j.attempt("collect", 2));
    let rows = rows_of(&p.db, TABLE);
    assert_eq!(rows.len(), 2, "{rows:#?}");
    let reader = Reader::open(&p.db).unwrap();
    let first = row_written_by(&reader, &one.id, 1);
    let second = row_written_by(&reader, &two.id, 1);
    assert_ne!(first.row_id, second.row_id);
    assert_eq!((first.call_seq, second.call_seq), (Some(1), Some(1)));
    assert_eq!(data_of(&first.data), json!({"ref": "first-attempt"}));
    assert_eq!(data_of(&second.data), json!({"ref": "second-attempt"}));
    assert_eq!(j.entries("row.written").len(), 2);
}

/// Row 19, over a whole run: an insert, a bad row, an update, a stale
/// update, then a call to another table (refused; the step fails). Every
/// `succeeded` `table_write` call has exactly one after-image at its key,
/// naming it; every `failed` or `refused` call has none; and every
/// after-image of the table names a `succeeded` call.
#[tokio::test]
async fn a_succeeded_call_always_has_its_after_image() {
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            write(insert(json!({"ref": "r-1"}))),
            write(insert(json!({"ref": 7}))),
            write_with(|r| update(&newest_row(r, TABLE), 1, json!({"title": "t"}))),
            write_with(|r| update(&newest_row(r, TABLE), 1, json!({"title": "late"}))),
            write(insert_into(OTHER_TABLE, json!({"ref": "x"}))),
        ],
    );
    let outcome = run(&p, &plan(vec![writing(step("collect", model))])).await;
    assert_eq!(outcome.status, "failed", "{outcome:?}");

    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);
    let calls = j.tool_calls_of(&attempt.id);
    let statuses: Vec<&str> = calls.iter().map(|c| c.status.as_str()).collect();
    assert_eq!(
        statuses,
        ["succeeded", "failed", "succeeded", "failed", "refused"]
    );
    let reader = Reader::open(&p.db).unwrap();
    for call in &calls {
        let image = reader.row_written_at(&attempt.id, call.call_seq).unwrap();
        match call.status.as_str() {
            "succeeded" => {
                let image = image.expect("a succeeded call has its after-image");
                assert_eq!(image.tool_call_id.as_deref(), Some(call.id.as_str()));
            }
            _ => assert!(image.is_none(), "{call:?}: {image:?}"),
        }
    }
    let row = only_row(&p.db, TABLE);
    let succeeded: Vec<&str> = calls
        .iter()
        .filter(|c| c.status == "succeeded")
        .map(|c| c.id.as_str())
        .collect();
    for image in history_of(&p.db, TABLE, &row.row_id) {
        let id = image.tool_call_id.clone().unwrap_or_default();
        assert!(succeeded.contains(&id.as_str()), "{image:?}");
    }
}
