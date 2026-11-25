//! Trust test (AD-10 as amended 2026-11-24, AD-11; epic 3 story 6, owner
//! condition Q4, matrix rows 35 and 36): the planted-string test of the
//! engine result.
//!
//! A successful `table_write` comes back as an `EngineResult`, which
//! ToolGate alone builds from engine values: the row id the engine minted,
//! the version that write produced and one fixed status word. Canaries
//! planted in a row's data value, in an unknown argument key, in an
//! unknown argument's value (the arguments text) and in an update's patch
//! reach the row (so they were really sent), and never the result: its
//! text is exactly the text `EngineResult::row_written` gives for the
//! committed row id, version and status. The result is no tool result, so
//! the attempt stays clean: its row, every reservation's taint and its next
//! row are clean, and the step after it gets its output as a trusted input.
//!
//! Every error result is excluded: a stale version or data the table's
//! columns refuse comes back as `Part::ToolResult` with `UntrustedData`
//! (`tool:table_write`), and the attempt is tainted from its next call, so
//! a row it writes after that is tainted.
//!
//! A live model names the row the engine minted; no network, no real
//! model. Written before `EngineResult` exists (every result is
//! `UntrustedData` today).
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

use am_domain::rows::{RowStatus, RowWritten};
use am_domain::{EngineResult, Taint};
use am_engine::run::{RunOutcome, StepPlan, run_plan};
use serde_json::{Value, json};
use support::*;
use tokio_util::sync::CancellationToken;

/// In a data value of the insert.
const DATA_CANARY: &str = "canary-data-value-7d21";
/// An unknown top-level argument key.
const KEY_CANARY: &str = "canary_key_4e9a";
/// An unknown argument's value: in the call's arguments text only.
const ARGS_CANARY: &str = "canary-args-text-91f0";
/// In the update's patch.
const PATCH_CANARY: &str = "canary-patch-value-b3c5";

const CANARIES: [&str; 4] = [DATA_CANARY, KEY_CANARY, ARGS_CANARY, PATCH_CANARY];

async fn run(p: &Project, steps: Vec<StepPlan>) -> RunOutcome {
    run_plan(
        &p.store,
        owner(),
        "manual",
        &plan(steps),
        CancellationToken::new(),
    )
    .await
    .unwrap()
}

/// `args` with the unknown key and the unknown argument's value planted.
fn planted(mut args: Value) -> Value {
    args[KEY_CANARY] = json!(1);
    args["note"] = json!(ARGS_CANARY);
    args
}

/// The one engine result for the call `call_id` in `request`.
fn result_for(request: &am_domain::ChatRequest, call_id: &str) -> Vec<EngineResult> {
    engine_results(request)
        .into_iter()
        .filter(|(id, _)| id == call_id)
        .map(|(_, r)| r)
        .collect()
}

/// Row 35: an insert with the data canary and an update with the patch
/// canary, both with the unknown key and the unknown argument planted. The
/// canaries are in the row; each next request holds an `EngineResult` for
/// the call (labelled `tool:table_write`), equal to the engine's own for
/// the committed row id, version and status (`inserted` then `updated`),
/// naming the row id and the version, and holding no canary. No tool
/// result is sent.
#[tokio::test]
async fn a_write_result_holds_only_engine_values() {
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
            say("done"),
        ],
    );

    let outcome = run(&p, vec![writing(step("collect", model.clone()))]).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let row = only_row(&p.db, TABLE);
    assert_eq!(row.version, 2);
    assert_eq!(
        data_of(&row.data)["title"],
        PATCH_CANARY,
        "the canary reached the store"
    );
    let images = history_of(&p.db, TABLE, &row.row_id);
    assert_eq!(data_of(&images[0].data)["title"], DATA_CANARY);
    let row_id = uuid::Uuid::parse_str(&row.row_id).expect("a UUID the engine minted");

    let requests = model.requests();
    assert_eq!(requests.len(), 3, "three turns");
    for (n, version, status) in [(1, 1, RowStatus::Inserted), (2, 2, RowStatus::Updated)] {
        let found = result_for(&requests[n], "call-1");
        let result = found
            .last()
            .unwrap_or_else(|| panic!("request {n} holds the engine result: {:#?}", requests[n]));
        let expected = EngineResult::row_written(&RowWritten {
            row_id,
            version,
            status,
        });
        assert_eq!(result.label(), WRITE_LABEL);
        assert_eq!(
            result.text(),
            expected.text(),
            "built from the row id, the version and the status only"
        );
        let text = result.text();
        assert!(text.contains(&row.row_id), "names the row: {text}");
        assert!(
            holds_version(text, version),
            "names version {version}: {text}"
        );
        for canary in CANARIES {
            assert!(!text.contains(canary), "`{canary}` in the result: {text}");
        }
    }
    for request in &requests {
        assert!(
            tool_results(request).is_empty(),
            "a successful write sends no tool result: {request:#?}"
        );
    }
}

/// Row 35: two successful writes leave the attempt clean: its taint at
/// rest, every `model_call.reserved` entry's taint, both rows and their
/// after-images are clean, and the next step gets the step's output as a
/// trusted input (a clean producer), not fenced.
#[tokio::test]
async fn a_successful_write_leaves_the_attempt_clean() {
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            write(insert(json!({ "ref": "r-1", "title": DATA_CANARY }))),
            write(insert(json!({ "ref": "r-2" }))),
            say("two rows recorded"),
        ],
    );
    let next = texts(&["read the summary"]);

    let outcome = run(
        &p,
        vec![
            writing(step("collect", model.clone())),
            step("next", next.clone()),
        ],
    )
    .await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);
    assert_eq!(attempt.taint, Taint::Clean, "the writes left it clean");
    let reserved = j.entries("model_call.reserved");
    assert!(reserved.len() >= 3, "{reserved:#?}");
    for entry in &reserved {
        assert!(audited_clean(&entry["args"]["taint"]), "{entry}");
    }
    let rows = rows_of(&p.db, TABLE);
    assert_eq!(rows.len(), 2, "{rows:#?}");
    for row in &rows {
        assert_eq!(row.taint, Taint::Clean, "{row:?}");
        for image in history_of(&p.db, TABLE, &row.row_id) {
            assert_eq!(image.taint, Taint::Clean, "{image:?}");
        }
    }
    let parts = user_parts(&next.requests()[0]);
    assert_eq!(
        trusted(&parts, "step:collect@1").text(),
        "two rows recorded",
        "a clean producer's output is a trusted input"
    );
}

/// Row 36: error results stay untrusted and taint. A stale update (the row
/// is at version 2, the call says 1), or data the columns refuse (an
/// undeclared column), comes back as `Part::ToolResult` with
/// `UntrustedData` labelled `tool:table_write`; the tool call is `failed`;
/// the attempt is tainted from its next call (its reservation says so) and
/// the row it inserts after that is tainted, row and after-image. Rows
/// written before the error stay as they were.
#[tokio::test]
async fn an_error_result_stays_untrusted_and_taints() {
    // A stale version.
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            write(insert(json!({ "ref": "r-1" }))),
            write_with(|r| update(&newest_row(r, TABLE), 1, json!({ "title": "second" }))),
            write_with(|r| update(&newest_row(r, TABLE), 1, json!({ "title": "stale" }))),
            write(insert(json!({ "ref": "r-2" }))),
            say("done"),
        ],
    );
    let outcome = run(&p, vec![writing(step("collect", model.clone()))]).await;
    assert_eq!(outcome.status, "succeeded", "the step goes on: {outcome:?}");
    let requests = model.requests();
    let errors = tool_results(&requests[3]);
    assert_eq!(errors.len(), 1, "{:#?}", requests[3]);
    let (call_id, error) = &errors[0];
    assert_eq!(call_id, "call-1");
    assert_eq!(error.label(), WRITE_LABEL);
    assert!(
        text_of(error).contains("version 2"),
        "the stale refusal's words: {}",
        text_of(error)
    );
    assert!(
        result_for(&requests[3], "call-1").len() == 2,
        "the two earlier writes stay engine results"
    );
    assert_tainted_after_error(&p, 3, "r-2");
    let first = rows_of(&p.db, TABLE)
        .into_iter()
        .find(|r| data_of(&r.data)["ref"] == "r-1")
        .unwrap();
    assert_eq!((first.version, first.taint), (2, Taint::Clean));
    assert_eq!(data_of(&first.data)["title"], "second");

    // Data the columns refuse.
    let p = project();
    let model = Live::new(
        &p.db,
        vec![
            write(insert(json!({ "ref": "r-1", "undeclared_column": 1 }))),
            write(insert(json!({ "ref": "r-2" }))),
            say("done"),
        ],
    );
    let outcome = run(&p, vec![writing(step("collect", model.clone()))]).await;
    assert_eq!(outcome.status, "succeeded", "the step goes on: {outcome:?}");
    let errors = tool_results(&model.requests()[1]);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].1.label(), WRITE_LABEL);
    assert!(engine_results(&model.requests()[1]).is_empty());
    assert_tainted_after_error(&p, 1, "r-2");
}

/// After the error at tool call `failed_seq` of `collect`: that call is
/// `failed`, the attempt is tainted, the reservation of the next model
/// call (and only from then on) is tainted, and the row `after_ref`
/// inserted after it is tainted in the row and its after-image.
fn assert_tainted_after_error(p: &Project, failed_seq: usize, after_ref: &str) {
    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);
    assert_eq!(attempt.taint, Taint::Tainted, "the error result tainted it");
    let calls = j.tool_calls_of(&attempt.id);
    assert_eq!(calls[failed_seq - 1].status, "failed", "{calls:#?}");
    let reserved = j.entries("model_call.reserved");
    for (n, entry) in reserved.iter().enumerate() {
        if n < failed_seq {
            assert!(
                audited_clean(&entry["args"]["taint"]),
                "call {}: {entry}",
                n + 1
            );
        } else {
            assert!(
                audited_tainted(&entry["args"]["taint"]),
                "call {}: {entry}",
                n + 1
            );
        }
    }
    let row = rows_of(&p.db, TABLE)
        .into_iter()
        .find(|r| data_of(&r.data)["ref"] == after_ref)
        .unwrap_or_else(|| panic!("the row {after_ref} was written"));
    assert_eq!(row.taint, Taint::Tainted, "{row:?}");
    let images = history_of(&p.db, TABLE, &row.row_id);
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].taint, Taint::Tainted);
}
