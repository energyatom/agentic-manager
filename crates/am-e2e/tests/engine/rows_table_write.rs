//! Engine test (AD-8, AD-10 as amended 2026-11-24, AD-11, AD-21, AD-43;
//! epic 3 story 6, plan matrix rows 2 to 5, 14, 20 and 21, and the
//! ticket's verify): a FakeLlm-scripted step's `table_write` lands rows
//! with version and taint, and an after-image per change naming the run,
//! the step attempt and the seat.
//!
//! Through `run_plan` (the shipped ToolGate, with am-tools' `table_write`
//! as `agentic run` wires it): a step whose seat is granted the Tables
//! capability's tools and that declares `entries` is offered exactly
//! `table_write`; its insert and its update commit the row and its
//! after-image, finish the tool call `succeeded`, are audited with ids
//! only, and come back as the engine's result. A stale version, an unknown
//! row and a bad row are tool errors the model reads (untrusted) while the
//! step goes on; nothing changes. Rows a failed step wrote stay. A schema
//! repair offers no tool.
//!
//! No network, no real model: a `Live` model (its turns made from the
//! journal when it is called, so a turn can name the row id the engine
//! minted) and am-testkit's FakeLlm. Written before any of it exists.
#![forbid(unsafe_code)]

mod support;

use std::sync::{Arc, Mutex};

use am_domain::rows::{RowStatus, RowTool, RowWritten};
use am_domain::{Actor, EngineResult, Taint};
use am_engine::run::{RunOutcome, RunPlan, StepPlan, run_plan};
use am_store::{RowHistoryRecord, RowRecord};
use am_tools::table_write::TableWrite;
use serde_json::{Value, json};
use support::*;
use tokio_util::sync::CancellationToken;

async fn run(p: &Project, plan: &RunPlan) -> RunOutcome {
    run_plan(&p.store, owner(), "manual", plan, CancellationToken::new())
        .await
        .unwrap()
}

/// An audit argument as a number (the writer may store a number or its
/// decimal text).
fn number(arg: &Value) -> Option<i64> {
    arg.as_i64()
        .or_else(|| arg.as_str().and_then(|s| s.parse().ok()))
}

/// The engine's result for a committed write, as ToolGate builds it.
fn result_of(row_id: &str, version: i64, status: RowStatus) -> EngineResult {
    EngineResult::row_written(&RowWritten {
        row_id: uuid::Uuid::parse_str(row_id).unwrap(),
        version,
        status,
    })
}

/// Rows 2 and 3, the verify: an insert, then an update at version 1.
///
/// - Each request offers exactly am-tools' `table_write` spec for
///   `entries`.
/// - The row is at version 2, clean, its data the insert merged with the
///   update's column.
/// - Two after-images, version 1 (the insert's data) and version 2 (the
///   whole row), each naming the Seat actor, the run, the attempt, the
///   seat, the call sequence and the tool call it finished; both calls
///   `succeeded`.
/// - Two `row.written` entries, target `row:entries/<id>`, with the op,
///   version, call sequence and taint and no data.
/// - The model reads each result as the engine's (`Part::EngineResult`,
///   labelled `tool:table_write`, its text ToolGate's for that row id,
///   version and status), never as untrusted data; the attempt stays clean.
#[tokio::test]
async fn a_scripted_table_write_lands_rows_with_version_taint_and_history() {
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            write(insert(
                json!({"ref": "r-1", "title": "first", "amount": 2.5}),
            )),
            write_with(|r| update(&newest_row(r, TABLE), 1, json!({"title": "second"}))),
            say("recorded"),
        ],
    );
    let plan = plan(vec![writing(step("collect", model.clone()))]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let requests = model.requests();
    assert_eq!(requests.len(), 3);
    let spec = TableWrite::new().spec(&entries());
    for (n, request) in requests.iter().enumerate() {
        assert_eq!(request.tools, vec![spec.clone()], "request {n}");
    }
    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);
    assert_eq!(attempt.status, "succeeded");
    assert_eq!(
        attempt.taint,
        Taint::Clean,
        "a write leaves the attempt clean"
    );

    let row = only_row(&p.db, TABLE);
    assert_eq!(row.version, 2);
    assert_eq!(row.taint, Taint::Clean);
    assert_eq!(
        data_of(&row.data),
        json!({"ref": "r-1", "title": "second", "amount": 2.5})
    );
    let calls = j.tool_calls_of(&attempt.id);
    assert_eq!(calls.len(), 2, "{calls:#?}");
    for (n, call) in calls.iter().enumerate() {
        assert_eq!(
            (call.tool.as_str(), call.status.as_str(), call.call_seq),
            ("table_write", "succeeded", n as i64 + 1)
        );
    }
    let history = history_of(&p.db, TABLE, &row.row_id);
    assert_eq!(history.len(), 2, "{history:#?}");
    let seat = Actor::Seat {
        seat: "seat-collect".into(),
        run: uuid::Uuid::parse_str(&outcome.run_id).unwrap(),
        step: "collect".into(),
    };
    for (n, image) in history.iter().enumerate() {
        assert_eq!(image.version, n as i64 + 1);
        assert_eq!(image.taint, Taint::Clean);
        assert_eq!(image.actor, seat);
        assert_eq!(image.run_id.as_deref(), Some(outcome.run_id.as_str()));
        assert_eq!(image.step_run_id.as_deref(), Some(attempt.id.as_str()));
        assert_eq!(image.seat.as_deref(), Some("seat-collect"));
        assert_eq!(image.call_seq, Some(n as i64 + 1));
        assert_eq!(image.tool_call_id.as_deref(), Some(calls[n].id.as_str()));
        assert_eq!(image.table_id, TABLE);
        assert_eq!(image.row_id, row.row_id);
    }
    assert_eq!(history[0].op, "insert");
    assert_eq!(history[1].op, "update");
    assert_eq!(
        data_of(&history[0].data),
        json!({"ref": "r-1", "title": "first", "amount": 2.5})
    );
    assert_eq!(data_of(&history[1].data), data_of(&row.data));
    assert!(history[0].seq < history[1].seq);

    let written = j.entries("row.written");
    assert_eq!(written.len(), 2, "{written:#?}");
    for (n, entry) in written.iter().enumerate() {
        assert_eq!(entry["target"], format!("row:{TABLE}/{}", row.row_id));
        assert_eq!(entry["args"]["op"], ["insert", "update"][n]);
        assert_eq!(number(&entry["args"]["version"]), Some(n as i64 + 1));
        assert_eq!(number(&entry["args"]["call_seq"]), Some(n as i64 + 1));
        assert!(audited_clean(&entry["args"]["taint"]), "{entry}");
        let text = entry.to_string();
        for value in ["first", "second", "2.5", "r-1"] {
            assert!(!text.contains(value), "no row data in the audit: {text}");
        }
    }

    for (n, status) in [(1, RowStatus::Inserted), (2, RowStatus::Updated)] {
        let results = engine_results(&requests[n]);
        let latest = results.last().expect("an engine result");
        assert_eq!(latest.0, "call-1");
        assert_eq!(latest.1, result_of(&row.row_id, n as i64, status));
        assert_eq!(latest.1.label(), WRITE_LABEL);
        assert!(
            tool_results(&requests[n]).is_empty(),
            "no untrusted tool result: {:#?}",
            requests[n].messages
        );
    }
}

/// A row and its after-images, as read at one moment.
type Snapshot = (RowRecord, Vec<RowHistoryRecord>);

/// Row 4 (owner Q2): a stale update is refused in words and nothing
/// changes. Row A is updated to version 2, then updated again at version 1;
/// row B, at version 1, is updated at version 2. Each stale call is a
/// `failed` tool call whose result the model reads as untrusted data naming
/// the row and its current version; the step goes on and succeeds. Row A's
/// data, version, taint and after-images are exactly what they were just
/// before the stale call (version 1 still readable from its history, and
/// version 2 as the current row); row B stays at version 1 with one
/// after-image.
#[tokio::test]
async fn a_stale_version_is_a_tool_error_and_the_row_is_unchanged() {
    let p = project();
    let before: Arc<Mutex<Option<Snapshot>>> = Arc::new(Mutex::new(None));
    let seen = before.clone();
    let model = Live::new(
        &p.db,
        vec![
            write(insert(json!({"ref": "a", "title": "one"}))),
            write_with(|r| update(&newest_row(r, TABLE), 1, json!({"title": "two"}))),
            write_with(move |r| {
                let a = newest_row(r, TABLE);
                *seen.lock().unwrap() = Some((
                    r.table_row_by_id(TABLE, &a).unwrap().unwrap(),
                    r.row_history_of(TABLE, &a).unwrap(),
                ));
                update(&a, 1, json!({"title": "late"}))
            }),
            write(insert(json!({"ref": "b"}))),
            write_with(|r| update(&newest_row(r, TABLE), 2, json!({"title": "early"}))),
            say("done"),
        ],
    );
    let plan = plan(vec![writing(step("collect", model.clone()))]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "succeeded", "the step goes on: {outcome:?}");
    let rows = rows_of(&p.db, TABLE);
    assert_eq!(rows.len(), 2, "{rows:#?}");
    let (a, b) = (&rows[0], &rows[1]);
    let (a_then, a_history_then) = before.lock().unwrap().clone().expect("seen");
    assert_eq!(a, &a_then, "row A is as it was before the stale call");
    assert_eq!(a.version, 2);
    let a_history = history_of(&p.db, TABLE, &a.row_id);
    assert_eq!(a_history, a_history_then, "no after-image added");
    assert_eq!(a_history[0].version, 1, "version 1 stays readable");
    assert_eq!(
        data_of(&a_history[0].data),
        json!({"ref": "a", "title": "one"})
    );
    assert_eq!(b.version, 1);
    assert_eq!(history_of(&p.db, TABLE, &b.row_id).len(), 1);

    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);
    let statuses: Vec<String> = j
        .tool_calls_of(&attempt.id)
        .into_iter()
        .map(|t| t.status)
        .collect();
    assert_eq!(
        statuses,
        ["succeeded", "succeeded", "failed", "succeeded", "failed"]
    );
    let requests = model.requests();
    for (n, row, version) in [(3, a, 2), (5, b, 1)] {
        let results = tool_results(&requests[n]);
        let (call_id, data) = results.last().expect("an untrusted tool result");
        assert_eq!(call_id, "call-1");
        assert_eq!(data.label(), WRITE_LABEL);
        let words = text_of(data);
        assert!(words.contains(&row.row_id), "names the row: {words}");
        assert!(words.contains(&format!("version {version}")), "{words}");
        assert!(
            !words.contains("late") && !words.contains("early"),
            "{words}"
        );
    }
}

/// Row 5: an update naming a `row_id` that is not a UUID, or a UUID the
/// table does not hold, is a `failed` tool call whose untrusted result says
/// "a row this table does not hold" without echoing the id; nothing is
/// written and the step goes on.
#[tokio::test]
async fn an_unknown_row_is_a_tool_error_without_its_id() {
    const NOT_A_UUID: &str = "row-id-canary-77c1";
    let p = project();
    let unknown = uuid::Uuid::now_v7().to_string();
    let model = Live::new(
        &p.db,
        vec![
            write(update(NOT_A_UUID, 1, json!({"title": "x"}))),
            write(update(&unknown, 1, json!({"title": "x"}))),
            say("done"),
        ],
    );
    let plan = plan(vec![writing(step("collect", model.clone()))]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert!(rows_of(&p.db, TABLE).is_empty());
    let requests = model.requests();
    for (n, id) in [(1, NOT_A_UUID), (2, unknown.as_str())] {
        let results = tool_results(&requests[n]);
        let (_, data) = results.last().expect("an untrusted tool result");
        let words = text_of(data);
        assert!(words.contains("a row this table does not hold"), "{words}");
        assert!(!words.contains(id), "the id is not echoed: {words}");
    }
    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);
    assert!(
        j.tool_calls_of(&attempt.id)
            .iter()
            .all(|t| t.status == "failed")
    );
    assert!(j.entries("row.written").is_empty());
}

/// Row 14: a bad row (a column the table does not declare, a value of the
/// wrong type, a required column missing) is a `failed` tool call whose
/// untrusted result is worded from the table's columns and types, never
/// the model's keys or values; nothing is written.
#[tokio::test]
async fn a_bad_row_is_a_tool_error_in_words() {
    const KEY: &str = "column_canary_41aa";
    const VALUE: &str = "value-canary-41ab";
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            write(insert(json!({"ref": "r-1", KEY: 1}))),
            write(insert(json!({"ref": "r-1", "amount": VALUE}))),
            write(insert(json!({"title": VALUE}))),
            say("done"),
        ],
    );
    let plan = plan(vec![writing(step("collect", model.clone()))]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert!(rows_of(&p.db, TABLE).is_empty());
    let requests = model.requests();
    for (n, words_from_columns) in [
        (1, vec!["ref", "title"]),
        (2, vec!["amount", "number"]),
        (3, vec!["ref"]),
    ] {
        let results = tool_results(&requests[n]);
        let (_, data) = results.last().expect("an untrusted tool result");
        assert_eq!(data.label(), WRITE_LABEL);
        let words = text_of(data);
        for w in words_from_columns {
            assert!(words.contains(w), "request {n} names `{w}`: {words}");
        }
        assert!(!words.contains(KEY) && !words.contains(VALUE), "{words}");
    }
    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);
    let statuses: Vec<String> = j
        .tool_calls_of(&attempt.id)
        .into_iter()
        .map(|t| t.status)
        .collect();
    assert_eq!(statuses, ["failed", "failed", "failed"]);
}

/// Row 21: rows written before the step fails stay; their after-images
/// name the failed attempt. Here the step writes a row, then asks to write
/// another team's table, which ToolGate refuses (the step fails); nothing
/// is rolled back.
#[tokio::test]
async fn rows_of_a_failed_step_stay_and_name_its_attempt() {
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            write(insert(json!({"ref": "kept"}))),
            write(insert_into(OTHER_TABLE, json!({"ref": "never"}))),
            say("never reached"),
        ],
    );
    let plan = plan(vec![writing(step("collect", model.clone()))]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);
    assert_eq!(attempt.status, "failed");
    let row = only_row(&p.db, TABLE);
    let history = history_of(&p.db, TABLE, &row.row_id);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].step_run_id.as_deref(), Some(attempt.id.as_str()));
    assert!(rows_of(&p.db, OTHER_TABLE).is_empty());
    assert_eq!(model.calls(), 2);
}

/// Row 20 (story 3.5's repair): a writing step with an output schema whose
/// final message is invalid gets its one repair call, which offers no tool;
/// a tool call in the repair's reply fails the step and writes nothing.
/// The row the first turn wrote stays.
#[tokio::test]
async fn a_repair_call_offers_no_table_write() {
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            write(insert(json!({"ref": "first-turn"}))),
            say("not the schema's JSON"),
            write(insert(json!({"ref": "from-the-repair"}))),
        ],
    );
    let plan = plan(vec![with_schema(
        writing(step("collect", model.clone())),
        line_schema(),
    )]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let requests = model.requests();
    assert_eq!(requests.len(), 3, "two turns and the repair");
    assert_eq!(offered(&requests[0]), ["table_write"]);
    assert!(requests[2].tools.is_empty(), "{:?}", requests[2].tools);
    let row = only_row(&p.db, TABLE);
    assert_eq!(data_of(&row.data), json!({"ref": "first-turn"}));
    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);
    assert_eq!(
        j.tool_calls_of(&attempt.id).len(),
        1,
        "the repair ran no tool"
    );
}

// ------------------------------------------------- review round 1 (3.23)
//
// The ToolGate slice's review triage round 1 (manager, 2026-11-25), end to
// end through `run_plan` and am-tools' `table_write`:
// - R1: data the store refuses (a key repeated inside a JSON value; an
//   update whose merged row passes the byte or the column cap) is a tool
//   error the model reads as untrusted data, and the step goes on;
// - R6: a call with no `table`, a `table` that is not a string, arguments
//   that are not JSON or JSON that is no object is a tool error in the
//   tool's words, not a ToolGate refusal that fails the step;
// - R7: `null` means absent, for `row_id` (an insert) and for a column on
//   an insert (as on an update).
// Written before the patches.

/// A turn that asks for one `table_write` whose arguments text is `text`
/// as written (a key may repeat; it may be no JSON at all).
fn raw_write(text: &str) -> Turn {
    let text = text.to_string();
    Box::new(move |_, _| am_domain::ChatOutcome {
        text: "recording".into(),
        tool_calls: vec![am_domain::ToolCall {
            id: "call-1".into(),
            name: "table_write".into(),
            arguments_json: text.clone(),
        }],
        usage: None,
        stop: am_domain::StopReason::ToolUse,
    })
}

/// The one untrusted tool result for the call `call-1` in `request`: its
/// label and words.
fn error_in(request: &am_domain::ChatRequest) -> (String, String) {
    let found: Vec<(String, String)> = tool_results(request)
        .into_iter()
        .filter(|(id, _)| id == "call-1")
        .map(|(_, d)| (d.label().to_string(), text_of(&d)))
        .collect();
    found
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("an untrusted tool result: {:#?}", request.messages))
}

/// The statuses of `collect`'s tool calls, in call order.
fn statuses(p: &Project) -> Vec<String> {
    let j = journal(&p.db);
    j.tool_calls_of(&j.attempt("collect", 1).id)
        .into_iter()
        .map(|t| t.status)
        .collect()
}

/// R1 (a) and (b, bytes): a key repeated inside the `extra` column's
/// object, and an update whose merged row passes 64 KiB (a row of about
/// 40 KiB, a patch of about 30 KiB, each within the tool's 64 KiB), are
/// each a `failed` call whose untrusted result, labelled
/// `tool:table_write`, the model reads; nothing is written (the big row
/// stays at version 1 with its one after-image, the repeated-key row is
/// absent); the step goes on to write its next row and succeeds.
#[tokio::test]
async fn data_the_store_refuses_is_a_tool_error_and_the_step_goes_on() {
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            raw_write(
                r#"{"table":"entries","data":{"ref":"r-dup","extra":{"dup_key":1,"dup_key":2}}}"#,
            ),
            write(insert(
                json!({"ref": "big", "title": "b".repeat(40 * 1024)}),
            )),
            write_with(|r| {
                update(
                    &newest_row(r, TABLE),
                    1,
                    json!({"extra": "m".repeat(30 * 1024)}),
                )
            }),
            write(insert(json!({"ref": "after"}))),
            say("done"),
        ],
    );
    let plan = plan(vec![writing(step("collect", model.clone()))]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "succeeded", "the step goes on: {outcome:?}");
    let requests = model.requests();
    assert_eq!(requests.len(), 5, "every turn ran");
    for n in [1, 3] {
        let (label, words) = error_in(&requests[n]);
        assert_eq!(label, WRITE_LABEL, "request {n}");
        assert!(!words.contains("dup_key"), "request {n}: {words}");
        assert!(!words.contains("mmmmmmmm"), "request {n}: {words}");
    }
    assert_eq!(statuses(&p), ["failed", "succeeded", "failed", "succeeded"]);
    let rows = rows_of(&p.db, TABLE);
    let refs: Vec<Value> = rows
        .iter()
        .map(|r| data_of(&r.data)["ref"].clone())
        .collect();
    assert_eq!(
        refs,
        [json!("big"), json!("after")],
        "nothing else was written"
    );
    let big = &rows[0];
    assert_eq!(big.version, 1);
    assert!(
        data_of(&big.data).get("extra").is_none(),
        "{}",
        big.data.len()
    );
    assert_eq!(history_of(&p.db, TABLE, &big.row_id).len(), 1);
    assert_eq!(journal(&p.db).entries("row.written").len(), 2);
}

/// A table of team `core` with `n` optional text columns `c0`…
fn wide(n: usize) -> am_domain::rows::DeclaredTable {
    am_domain::rows::DeclaredTable {
        id: "wide".into(),
        team: TEAM.into(),
        columns: (0..n)
            .map(|i| am_domain::rows::ColumnSpec {
                id: format!("c{i}"),
                kind: am_domain::rows::ColumnKind::Text,
                required: false,
                values: vec![],
            })
            .collect(),
    }
}

/// `n` columns `c<from>`… as data, each holding `v`.
fn cells(from: usize, n: usize) -> Value {
    Value::Object(
        (from..from + n)
            .map(|i| (format!("c{i}"), json!("v")))
            .collect(),
    )
}

/// R1 (b, columns): on a table that declares 300 columns, an insert of 200
/// is written; an update adding 100 more (a merged row of 300, past the
/// store's 256) is a `failed` call whose untrusted result the model reads;
/// the row stays at version 1 with its 200 columns, and the step goes on.
#[tokio::test]
async fn a_merged_row_past_the_column_cap_is_a_tool_error() {
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            ask(vec![(
                "table_write",
                json!({"table": "wide", "data": cells(0, 200)}),
            )]),
            write_with(|r| {
                json!({
                    "table": "wide",
                    "row_id": newest_row(r, "wide"),
                    "expected_version": 1,
                    "data": cells(200, 100),
                })
            }),
            say("done"),
        ],
    );
    let collect = StepPlan {
        grants: tables_on(),
        table: Some(wide(300)),
        ..step("collect", model.clone())
    };

    let outcome = run(&p, &plan(vec![collect])).await;

    assert_eq!(outcome.status, "succeeded", "the step goes on: {outcome:?}");
    let requests = model.requests();
    assert_eq!(requests.len(), 3);
    let (label, words) = error_in(&requests[2]);
    assert_eq!(label, WRITE_LABEL);
    assert!(words.contains("column"), "{words}");
    assert_eq!(statuses(&p), ["succeeded", "failed"]);
    let row = only_row(&p.db, "wide");
    assert_eq!(row.version, 1);
    assert_eq!(data_of(&row.data).as_object().map(|o| o.len()), Some(200));
}

/// R6: four malformed calls, each a `failed` call whose untrusted result
/// (`tool:table_write`) names the declared table and echoes nothing the
/// model wrote: no `table`, a `table` that is not a string, arguments that
/// are not JSON, and a JSON array. None is a ToolGate refusal (no
/// `refused` call, no `toolgate.refused` entry); the step goes on to write
/// its row and succeeds. A `table` naming another table is still refused
/// at admission (`taint_rows_declared_table`, `toolgate.rs`).
#[tokio::test]
async fn malformed_arguments_are_tool_errors_and_the_step_goes_on() {
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            raw_write(r#"{"data":{"ref":"canary-mal-r6"}}"#),
            raw_write(r#"{"table":7,"data":{"ref":"canary-mal-r6"}}"#),
            raw_write("canary-mal-r6 is not JSON {"),
            raw_write(r#"["canary-mal-r6"]"#),
            write(insert(json!({"ref": "r-1"}))),
            say("done"),
        ],
    );
    let plan = plan(vec![writing(step("collect", model.clone()))]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "succeeded", "the step goes on: {outcome:?}");
    let requests = model.requests();
    assert_eq!(requests.len(), 6, "every turn ran");
    for (n, request) in requests.iter().enumerate().take(5).skip(1) {
        let (label, words) = error_in(request);
        assert_eq!(label, WRITE_LABEL, "request {n}");
        assert!(
            words.contains(TABLE),
            "request {n}: names the table: {words}"
        );
        assert!(!words.contains("canary-mal-r6"), "request {n}: {words}");
    }
    assert_eq!(
        statuses(&p),
        ["failed", "failed", "failed", "failed", "succeeded"]
    );
    let j = journal(&p.db);
    assert!(
        j.entries("toolgate.refused").is_empty(),
        "no ToolGate refusal"
    );
    assert_eq!(data_of(&only_row(&p.db, TABLE).data), json!({"ref": "r-1"}));
}

/// R7 (`row_id`): a call whose `row_id` is `null`, with no
/// `expected_version`, is an insert: the row lands at version 1 and the
/// model reads `inserted`, as for the same call without the key.
#[tokio::test]
async fn a_null_row_id_is_an_insert() {
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            raw_write(r#"{"table":"entries","row_id":null,"data":{"ref":"r-1"}}"#),
            say("done"),
        ],
    );
    let plan = plan(vec![writing(step("collect", model.clone()))]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let row = only_row(&p.db, TABLE);
    assert_eq!(
        (row.version, data_of(&row.data)),
        (1, json!({"ref": "r-1"}))
    );
    let results = engine_results(&model.requests()[1]);
    assert_eq!(
        results.last().map(|(_, r)| r.clone()),
        Some(result_of(&row.row_id, 1, RowStatus::Inserted))
    );
}

/// R7 (columns): a `null` column on an insert is absent, as `null` on an
/// update removes a column: the stored row, its after-image and the
/// `row.written` column count hold only the columns with a value.
#[tokio::test]
async fn a_null_column_on_an_insert_is_absent() {
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            write(insert(json!({"ref": "r-1", "title": null, "amount": 2}))),
            say("done"),
        ],
    );
    let plan = plan(vec![writing(step("collect", model.clone()))]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let row = only_row(&p.db, TABLE);
    assert_eq!(data_of(&row.data), json!({"ref": "r-1", "amount": 2}));
    let images = history_of(&p.db, TABLE, &row.row_id);
    assert_eq!(data_of(&images[0].data), json!({"ref": "r-1", "amount": 2}));
    let written = journal(&p.db).entries("row.written");
    assert_eq!(
        number(&written[0]["args"]["columns"]),
        Some(2),
        "{written:#?}"
    );
}
