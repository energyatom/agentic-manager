//! Engine test (AD-8; AD-10 as amended 2026-11-24 with Winston's wording,
//! "journaled on its tool call's journal row"; epic 3 story 6's ToolGate
//! slice, plan matrix row 44): the engine result of a successful
//! `table_write` is journaled on its tool call's row, and a resume (3.9) or
//! a drill-down (epic 6) rebuilds it from the writer's committed record.
//!
//! One step writes through the shipped ToolGate: an insert, an update, a
//! stale update (an untrusted error result; the step goes on) and a write
//! to another table (a ToolGate refusal; the step fails). Then:
//!
//! - each succeeded call's row is journaled `engine` / `tool:table_write`,
//!   with an after-image and a receipt at its key naming that row; the
//!   failed call `untrusted` / `tool:table_write`; the refused call none;
//! - `am_engine::toolgate::engine_result_of(reader, step_run_id,
//!   call_seq)`, on a fresh read-only connection after the store is closed,
//!   rebuilds for each succeeded call exactly the result the model
//!   received (the insert's version 1 although the row has moved on), and
//!   nothing for the failed, the refused or an unknown call;
//! - canaries planted in a data value, an unknown argument key, the
//!   arguments text and the update's patch reach the row (so they were
//!   sent), and never the tool-call journal rows (every column, as stored),
//!   the receipts, the rebuilt results or the audit export, nor a hash of
//!   them.
//!
//! No network, no real model: a `Live` model names the row the engine
//! minted. Written before `EngineResult`, the journaling or the rebuild
//! exist.
#![forbid(unsafe_code)]

mod support;

use std::path::{Path, PathBuf};

use am_domain::EngineResult;
use am_engine::run::{RunOutcome, run_plan};
use am_engine::toolgate::engine_result_of;
use am_store::Reader;
use serde_json::{Value, json};
use support::*;
use tokio_util::sync::CancellationToken;

/// In a data value of the insert.
const DATA_CANARY: &str = "canary-journal-data-5a10";
/// An unknown top-level argument key.
const KEY_CANARY: &str = "canary_journal_key_5a11";
/// An unknown argument's value: in the arguments text only.
const ARGS_CANARY: &str = "canary-journal-args-5a12";
/// In the update's patch.
const PATCH_CANARY: &str = "canary-journal-patch-5a13";

const CANARIES: [&str; 4] = [DATA_CANARY, KEY_CANARY, ARGS_CANARY, PATCH_CANARY];

/// `args` with the unknown key and the unknown argument's value planted.
fn planted(mut args: Value) -> Value {
    args[KEY_CANARY] = json!(1);
    args["note"] = json!(ARGS_CANARY);
    args
}

/// What one mixed run left: the run, the writing attempt's id, the model's
/// requests and the database's path (the store is closed).
struct Mixed {
    _dir: tempfile::TempDir,
    db: PathBuf,
    outcome: RunOutcome,
    step_run: String,
    requests: Vec<am_domain::ChatRequest>,
}

/// The step `collect` writes, one call per turn: call 1 inserts (planted),
/// call 2 updates at version 1 (planted, the patch canary), call 3 updates
/// at version 1 again (stale: an untrusted error, the step goes on), call 4
/// writes another team's table (refused by ToolGate: the step fails). The
/// store is closed before the caller reads anything.
async fn mixed() -> Mixed {
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            write(planted(insert(
                json!({ "ref": "r-1", "title": DATA_CANARY }),
            ))),
            write_with(|r| {
                planted(update(
                    &newest_row(r, TABLE),
                    1,
                    json!({ "title": PATCH_CANARY }),
                ))
            }),
            write_with(|r| update(&newest_row(r, TABLE), 1, json!({ "title": "stale" }))),
            write(insert_into(OTHER_TABLE, json!({ "ref": "r-2" }))),
            say("never reached"),
        ],
    );
    let outcome = run_plan(
        &p.store,
        owner(),
        "manual",
        &plan(vec![writing(step("collect", model.clone()))]),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let step_run = journal(&p.db).attempt("collect", 1).id;
    let Project { dir, db, store } = p;
    drop(store);
    Mixed {
        _dir: dir,
        db,
        outcome,
        step_run,
        requests: model.requests(),
    }
}

/// The engine result the model received for the write of turn `n` (the
/// last one in request `n`, which follows that turn).
fn sent(m: &Mixed, n: usize) -> EngineResult {
    engine_results(&m.requests[n])
        .last()
        .unwrap_or_else(|| panic!("request {n} holds an engine result: {:#?}", m.requests[n]))
        .1
        .clone()
}

/// Every column of every row of `table`, as text, read from the file.
fn stored_text(db: &Path, table: &str) -> String {
    let conn = rusqlite::Connection::open(db).unwrap();
    let mut stmt = conn.prepare(&format!("SELECT * FROM {table}")).unwrap();
    let columns = stmt.column_count();
    let mut rows = stmt.query([]).unwrap();
    let mut out = String::new();
    while let Some(row) = rows.next().unwrap() {
        for i in 0..columns {
            let value = match row.get_ref(i).unwrap() {
                rusqlite::types::ValueRef::Null => String::new(),
                rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                rusqlite::types::ValueRef::Real(v) => v.to_string(),
                rusqlite::types::ValueRef::Text(t) | rusqlite::types::ValueRef::Blob(t) => {
                    String::from_utf8_lossy(t).into_owned()
                }
            };
            out.push_str(&value);
            out.push('\u{1f}');
        }
        out.push('\n');
    }
    out
}

/// Row 44 (AD-8; AD-10 with Winston's wording): each result's kind and
/// label are journaled on its own tool call's row. Calls 1 and 2 are
/// `succeeded`, `engine` / `tool:table_write`, each with an after-image and
/// a receipt at its key that name that call's row; call 3 (stale) is
/// `failed`, `untrusted` / `tool:table_write`, with no receipt; call 4
/// (refused) has no kind, no label and no receipt. The run fails on the
/// refusal.
#[tokio::test]
async fn each_result_kind_is_journaled_with_its_label() {
    let m = mixed().await;

    assert_eq!(m.outcome.status, "failed", "{:?}", m.outcome);
    let reader = Reader::open(&m.db).unwrap();
    let calls = reader.tool_calls(&m.step_run).unwrap();
    let shape: Vec<(i64, &str, Option<&str>, Option<&str>)> = calls
        .iter()
        .map(|c| {
            (
                c.call_seq,
                c.status.as_str(),
                c.result_kind.as_deref(),
                c.result_label.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        vec![
            (1, "succeeded", Some("engine"), Some(WRITE_LABEL)),
            (2, "succeeded", Some("engine"), Some(WRITE_LABEL)),
            (3, "failed", Some("untrusted"), Some(WRITE_LABEL)),
            (4, "refused", None, None),
        ],
        "{calls:#?}"
    );
    for call in &calls[..2] {
        let receipt = reader
            .row_receipt(&m.step_run, call.call_seq)
            .unwrap()
            .unwrap_or_else(|| panic!("a receipt for call {}", call.call_seq));
        assert_eq!(receipt.tool_call_id, call.id, "on its own call's row");
        assert_eq!(receipt.call_seq, call.call_seq);
        let image = row_written_by(&reader, &m.step_run, call.call_seq);
        assert_eq!(image.tool_call_id.as_deref(), Some(call.id.as_str()));
        assert_eq!(image.version, receipt.version);
    }
    for seq in [3, 4] {
        assert_eq!(
            reader.row_receipt(&m.step_run, seq).unwrap(),
            None,
            "call {seq} wrote no row"
        );
    }
}

/// Row 44 (AD-10 with Winston's wording; owner: rebuilt, never parsed):
/// after the store is closed, `engine_result_of` on a fresh read-only
/// connection rebuilds, for each succeeded call, the very engine result the
/// model received after it: the insert's says version 1 and `inserted`
/// although the row is now at version 2, the update's version 2. A
/// rebuild is the same each time. The failed call, the refused call, a
/// call sequence never journaled and an unknown step run rebuild nothing.
#[tokio::test]
async fn a_resume_rebuilds_the_same_result_from_the_record() {
    let m = mixed().await;
    let reader = Reader::open(&m.db).unwrap();
    let row = only_row(&m.db, TABLE);
    assert_eq!(row.version, 2, "the row has moved on: {row:?}");

    for (seq, version) in [(1, 1), (2, 2)] {
        let sent = sent(&m, seq as usize);
        let rebuilt = engine_result_of(&reader, &m.step_run, seq)
            .unwrap()
            .unwrap_or_else(|| panic!("call {seq} is rebuilt"));
        assert_eq!(rebuilt, sent, "call {seq}: the result the model received");
        assert_eq!(rebuilt.text(), sent.text(), "call {seq}");
        assert_eq!(rebuilt.label(), WRITE_LABEL);
        assert!(rebuilt.text().contains(&row.row_id), "{}", rebuilt.text());
        assert!(
            holds_version(rebuilt.text(), version),
            "call {seq} names version {version}: {}",
            rebuilt.text()
        );
        assert_eq!(
            engine_result_of(&reader, &m.step_run, seq).unwrap(),
            Some(rebuilt),
            "a rebuild is the same each time"
        );
    }
    assert_ne!(sent(&m, 1).text(), sent(&m, 2).text());
    for seq in [3, 4, 5] {
        assert_eq!(
            engine_result_of(&reader, &m.step_run, seq).unwrap(),
            None,
            "call {seq} has no engine result"
        );
    }
    assert_eq!(
        engine_result_of(&reader, "no-such-step-run", 1).unwrap(),
        None
    );
}

/// Row 44 with row 35 (owner Q4): the canaries reached the store (the
/// insert's after-image holds the data canary, the row the patch canary),
/// and none of them, nor its sha256, is in any column of the tool-call
/// journal rows as stored, in the receipts, in the rebuilt results or in
/// the audit export.
#[tokio::test]
async fn the_journal_and_the_rebuild_hold_no_canary() {
    let m = mixed().await;
    let row = only_row(&m.db, TABLE);
    assert_eq!(
        data_of(&row.data)["title"],
        PATCH_CANARY,
        "the patch was sent"
    );
    let images = history_of(&m.db, TABLE, &row.row_id);
    assert_eq!(
        data_of(&images[0].data)["title"],
        DATA_CANARY,
        "the data was sent"
    );

    let reader = Reader::open(&m.db).unwrap();
    let mut seen = vec![(
        "the tool-call journal rows".to_string(),
        stored_text(&m.db, "tool_calls"),
    )];
    for seq in [1, 2] {
        let receipt = reader.row_receipt(&m.step_run, seq).unwrap().unwrap();
        seen.push((format!("receipt {seq}"), format!("{receipt:?}")));
        seen.push((
            format!("receipt {seq}'s fields"),
            [
                receipt.table_id.clone(),
                receipt.row_id.clone(),
                receipt.op.clone(),
                receipt.step_run_id.clone(),
                receipt.tool_call_id.clone(),
            ]
            .join(" "),
        ));
        let rebuilt = engine_result_of(&reader, &m.step_run, seq)
            .unwrap()
            .unwrap();
        seen.push((
            format!("rebuilt result {seq}"),
            format!("{} {} {rebuilt:?}", rebuilt.label(), rebuilt.text()),
        ));
    }
    seen.push((
        "the audit export".to_string(),
        audit(&reader)
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    ));
    for (what, text) in &seen {
        assert!(!text.is_empty(), "{what} was read");
        for canary in CANARIES {
            assert!(!text.contains(canary), "`{canary}` in {what}: {text}");
            let hash = am_store::audit::sha256_hex(canary);
            assert!(!text.contains(&hash), "a hash of `{canary}` in {what}");
        }
    }
}
