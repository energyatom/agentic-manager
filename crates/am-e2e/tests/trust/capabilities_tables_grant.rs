//! Trust test (AD-11, AD-46; epic 3 story 6, matrix rows 1, 6, 8, 9, 11
//! and 34): a capability that is off means its tool is absent and refused,
//! and every tool request goes through ToolGate.
//!
//! - `table_write` reaches a seat only through its Tables capability (the
//!   plan's `grants`): a step whose seat holds it and that declares a table
//!   is offered exactly am-tools' `table_write` spec for that table; a step
//!   with Tables off, or with no `table:`, is offered nothing, and a call
//!   it makes anyway is refused by ToolGate in words naming the seat (or
//!   the step), journaled `refused` and audited `toolgate.refused`; no row
//!   is written.
//! - A tool no manifest holds is refused and audited only by its label
//!   (`unknown:<sha256>`), never by the model's name.
//! - On the shipped path (`run_plan`, and `agentic run` on the
//!   `rows-documents` fixture) every request, whatever it names, becomes a
//!   tool call row and a ToolGate decision; story 3.1's "tools are not
//!   available" path is gone.
//!
//! FakeLlm only; no network, no real model. Written before the grants
//! check, the shipped gate or `table_write` exist.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

use std::sync::Arc;

use am_domain::rows::RowTool;
use am_domain::{InboxKind, LlmProvider};
use am_engine::run::{RunOutcome, StepPlan, run_plan};
use am_testkit::{FakeLlm, ScriptedTurn};
use am_tools::table_write::TableWrite;
use serde_json::{Value, json};
use support::*;
use tokio_util::sync::CancellationToken;

/// A model that asks once for `name` with `args`, then says it is done.
fn asks(name: &str, args: &Value) -> Arc<FakeLlm> {
    Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("recording").tool_call(name, &args.to_string()),
        ScriptedTurn::text("done"),
    ]))
}

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

/// The target ToolGate audits a refusal of `tool` in `step` of `run` under.
fn target(tool: &str, run: &str, step: &str) -> String {
    format!("tool:{tool} run:{run} step:{step}")
}

/// One refused tool call of `tool` on the only attempt of `key`, one
/// `toolgate.refused` entry for it naming the run and the step, the step
/// and the run failed with no Failure item (3.3's rule for a refusal), and
/// no row in either table. Returns the refusal's words.
fn assert_refused_by_toolgate(p: &Project, outcome: &RunOutcome, key: &str, tool: &str) -> String {
    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let j = journal(&p.db);
    let attempt = j.attempt(key, 1);
    assert_eq!(attempt.status, "failed");
    let calls = j.tool_calls_of(&attempt.id);
    assert_eq!(calls.len(), 1, "{calls:#?}");
    assert_eq!(calls[0].status, "refused");
    assert_eq!(calls[0].tool, tool);
    let refusals = j.entries("toolgate.refused");
    assert_eq!(refusals.len(), 1, "{refusals:#?}");
    assert_eq!(refusals[0]["target"], target(tool, &outcome.run_id, key));
    assert!(
        j.items.iter().all(|i| i.kind != InboxKind::Failure),
        "a ToolGate refusal raises no Failure item: {:#?}",
        j.items
    );
    assert!(rows_of(&p.db, TABLE).is_empty(), "no row in {TABLE}");
    assert!(
        rows_of(&p.db, OTHER_TABLE).is_empty(),
        "no row in {OTHER_TABLE}"
    );
    let words = outcome.reason.clone().expect("the refusal in words");
    assert!(
        !words.contains("tools are not available"),
        "3.1's words are gone: {words}"
    );
    words
}

/// Rows 1 and 8: with Tables on and `table: entries` declared, the step is
/// offered exactly `table_write`, whose spec is am-tools' for `entries`
/// (the table's id, its column ids and types). With Tables on and no
/// table, or a table declared and Tables off, nothing is offered; the call
/// a seat without the grant makes anyway is refused in words naming the
/// seat and `not granted`, journaled and audited, and writes no row.
#[tokio::test]
async fn tables_off_means_table_write_absent_and_refused() {
    // Tables on, the table declared: offered, and only it.
    let p = project();
    let llm = texts(&["nothing to record"]);
    let outcome = run(&p, vec![writing(step("collect", llm.clone()))]).await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let request = &llm.requests()[0];
    assert_eq!(
        request.tools,
        vec![TableWrite::new().spec(&entries())],
        "exactly table_write, built from the declared table"
    );
    let schema = &request.tools[0].input_schema_json;
    for word in [TABLE, "ref", "title", "amount", "count", "done", "state"] {
        assert!(schema.contains(word), "the spec names `{word}`: {schema}");
    }

    // Tables on, no table declared: nothing offered.
    let p = project();
    let llm = texts(&["nothing to record"]);
    let granted = StepPlan {
        grants: tables_on(),
        ..step("collect", llm.clone())
    };
    let outcome = run(&p, vec![granted]).await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert!(offered(&llm.requests()[0]).is_empty(), "no table, no tool");

    // A table declared, Tables off: nothing offered, and a call is refused.
    let p = project();
    let llm = asks("table_write", &insert(json!({ "ref": "r-1" })));
    let off = StepPlan {
        grants: Default::default(),
        ..writing(step("collect", llm.clone()))
    };
    let outcome = run(&p, vec![off]).await;
    assert!(
        offered(&llm.requests()[0]).is_empty(),
        "absent when the capability is off: {:?}",
        llm.requests()[0].tools
    );
    let words = assert_refused_by_toolgate(&p, &outcome, "collect", "table_write");
    assert!(words.contains("not granted"), "{words}");
    assert!(words.contains("seat-collect"), "names the seat: {words}");
    assert_eq!(llm.calls(), 1, "the step ends at the refusal");
}

/// Row 9: a tool no manifest holds is refused, journaled and audited by
/// its label `unknown:<sha256 of the name>`; the model's name is in no
/// tool call row, no audit entry and not in the words.
#[tokio::test]
async fn an_unknown_tool_is_refused_and_audited_by_label() {
    const NAME: &str = "canary_tool_name_5c0e";
    let p = project();
    let llm = asks(NAME, &json!({}));

    let outcome = run(&p, vec![writing(step("collect", llm.clone()))]).await;

    let label = format!("unknown:{}", am_store::audit::sha256_hex(NAME));
    let words = assert_refused_by_toolgate(&p, &outcome, "collect", &label);
    assert!(words.contains("unknown"), "{words}");
    assert!(!words.contains(NAME), "{words}");
    let j = journal(&p.db);
    let export = serde_json::to_string(&j.audit).unwrap();
    assert!(!export.contains(NAME), "the raw name is never audited");
    assert!(export.contains(&label));
}

/// Rows 6, 9 and 34 (the shipped gate): on `run_plan`, every tool request
/// becomes a tool call row and a ToolGate decision, whatever it names: an
/// unknown tool, a built-in the seat was not granted, a granted local-write
/// tool this version does not run, `table_write` to another table, and
/// `table_write` from a step with no grants at all. A granted write to the
/// declared table goes through the same gate and lands. Through `agentic
/// run` on the fixture, a step that declares no table is refused by
/// ToolGate too, while the writing step's row lands.
#[tokio::test]
async fn every_tool_request_goes_through_toolgate() {
    let refused: Vec<(&str, StepPlan, String)> = vec![
        (
            "an unknown tool",
            writing(step("collect", asks("no_such_tool", &json!({})))),
            format!("unknown:{}", am_store::audit::sha256_hex("no_such_tool")),
        ),
        (
            "a built-in not granted",
            writing(step("collect", asks("web_search", &json!({"query": "x"})))),
            "web_search".to_string(),
        ),
        (
            "a granted local-write tool this version does not run",
            StepPlan {
                grants: ["write_document".to_string()].into_iter().collect(),
                ..writing(step(
                    "collect",
                    asks("write_document", &json!({"text": "x"})),
                ))
            },
            "write_document".to_string(),
        ),
        (
            "table_write to another table",
            writing(step(
                "collect",
                asks(
                    "table_write",
                    &insert_into(OTHER_TABLE, json!({ "ref": "r-1" })),
                ),
            )),
            "table_write".to_string(),
        ),
        (
            "table_write with no grants",
            step(
                "collect",
                asks("table_write", &insert(json!({ "ref": "r-1" }))),
            ),
            "table_write".to_string(),
        ),
    ];
    for (what, collect, tool) in refused {
        let p = project();
        let outcome = run(&p, vec![collect]).await;
        let words = assert_refused_by_toolgate(&p, &outcome, "collect", &tool);
        assert!(words.contains("refused"), "{what}: {words}");
    }

    // The control: a granted write to the declared table goes through the
    // gate too, and lands.
    let p = project();
    let outcome = run(
        &p,
        vec![writing(step(
            "collect",
            asks("table_write", &insert(json!({ "ref": "r-1" }))),
        ))],
    )
    .await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let calls = j.tool_calls_of(&j.attempt("collect", 1).id);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].status, "succeeded");
    assert_eq!(rows_of(&p.db, TABLE).len(), 1);
    assert!(j.entries("toolgate.refused").is_empty());

    // Row 34: the shipped `agentic run` on the fixture.
    let cli = cli();
    let root = cli.copy_of("rows-documents");
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("recording")
            .tool_call("table_write", &insert(json!({ "ref": "r-1" })).to_string()),
        ScriptedTurn::text("recorded"),
        ScriptedTurn::text("recording again")
            .tool_call("table_write", &insert(json!({ "ref": "r-2" })).to_string()),
        ScriptedTurn::text("never reached"),
    ]));
    let ran = cli
        .run(&root, "record", local(llm.clone() as Arc<dyn LlmProvider>))
        .await;
    let (run_id, status) = ran.run_line();
    assert_eq!(status, "failed", "{}", ran.all());
    let db = cli.db(&root);
    let j = journal(&db);
    let collect = j.attempt("collect", 1);
    assert_eq!(collect.status, "succeeded");
    assert_eq!(
        j.tool_calls_of(&collect.id)
            .iter()
            .map(|t| (t.tool.as_str(), t.status.as_str()))
            .collect::<Vec<_>>(),
        vec![("table_write", "succeeded")]
    );
    let plain = j.attempt("plain", 1);
    assert_eq!(plain.status, "failed");
    assert_eq!(
        j.tool_calls_of(&plain.id)
            .iter()
            .map(|t| (t.tool.as_str(), t.status.as_str()))
            .collect::<Vec<_>>(),
        vec![("table_write", "refused")]
    );
    let refusals = j.entries("toolgate.refused");
    assert_eq!(refusals.len(), 1, "{refusals:#?}");
    assert_eq!(
        refusals[0]["target"],
        target("table_write", &run_id, "plain")
    );
    assert!(
        refusals[0]["outcome"]
            .as_str()
            .unwrap_or_default()
            .contains("declares no table"),
        "{:#?}",
        refusals[0]
    );
    let rows = rows_of(&db, TABLE);
    assert_eq!(rows.len(), 1, "only the writing step's row: {rows:#?}");
    assert_eq!(data_of(&rows[0].data)["ref"], "r-1");
    assert!(
        offered(&llm.requests()[2]).is_empty(),
        "the step with no table is offered nothing"
    );
}
