//! Trust test (AD-10 (b), AD-11, AD-21, RULE-8's binding; epic 3 story 6,
//! matrix rows 6, 7 and 12, and the ticket's verify): a seat writes only
//! the table its step declares, and only when that table belongs to the
//! seat's team, whether the step is clean or tainted.
//!
//! - A `table_write` that names another table than the declared one is
//!   refused by ToolGate: the tool call is `refused`, `toolgate.refused` is
//!   audited (the Seat actor, `tool:table_write run:<run> step:<key>`), the
//!   words name the declared table and never the model's table text, the
//!   step fails, and no row lands anywhere.
//! - A step that declares another team's table is refused before any row:
//!   `run_plan` refuses the plan, or (if a run exists) offers no
//!   `table_write` and ToolGate refuses the call; no row in either table.
//! - A tainted step writes its declared table (the row and its after-image
//!   carry taint 1), and its write to another table is refused the same
//!   way, the refusal's audit args carrying the taint.
//!
//! Not `rule_08_*`: story 4.9 writes the RULE-8 suite, failing first there.
//! FakeLlm only; no network, no real model. Written before the binding.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

use std::sync::Arc;

use am_domain::{Actor, InboxKind, Taint};
use am_engine::run::{RunOutcome, StepPlan, run_plan};
use am_store::Reader;
use am_testkit::{FakeLlm, ScriptedTurn};
use serde_json::{Value, json};
use support::*;
use tokio_util::sync::CancellationToken;

/// A table name the model makes up: it must never be echoed or audited.
const MADE_UP_TABLE: &str = "canary_table_name_6f1d";

/// A model that asks for `table_write` with each of `writes` in turn (one
/// per turn), then says it is done.
fn writes(writes: &[Value]) -> Arc<FakeLlm> {
    let mut turns: Vec<ScriptedTurn> = writes
        .iter()
        .map(|args| ScriptedTurn::text("recording").tool_call("table_write", &args.to_string()))
        .collect();
    turns.push(ScriptedTurn::text("done"));
    Arc::new(FakeLlm::new(turns))
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

/// Row 6 (the verify): `table_write` to `others` (a real table of another
/// team) or to a name the model made up, from a step that declares
/// `entries`: refused, journaled and audited by the Seat; the words name
/// `entries` and never the model's text; the step and the run fail with no
/// Failure item; no row in either table; the model is not called again.
#[tokio::test]
async fn a_write_to_another_table_is_refused_and_audited() {
    for named in [OTHER_TABLE, MADE_UP_TABLE] {
        let p = project();
        let llm = writes(&[insert_into(named, json!({ "ref": "r-1" }))]);

        let outcome = run(&p, vec![writing(step("collect", llm.clone()))]).await;

        assert_eq!(outcome.status, "failed", "{named}: {outcome:?}");
        let words = outcome.reason.clone().expect("in words");
        assert!(
            words.contains(TABLE),
            "{named}: names the declared table: {words}"
        );
        assert!(words.contains("another table"), "{named}: {words}");
        assert!(!words.contains(MADE_UP_TABLE), "{named}: {words}");
        assert_eq!(llm.calls(), 1, "{named}: the step ends at the refusal");

        let j = journal(&p.db);
        let attempt = j.attempt("collect", 1);
        assert_eq!(attempt.status, "failed", "{named}");
        let calls = j.tool_calls_of(&attempt.id);
        assert_eq!(calls.len(), 1, "{named}: {calls:#?}");
        assert_eq!(
            (calls[0].tool.as_str(), calls[0].status.as_str()),
            ("table_write", "refused"),
            "{named}"
        );
        let refusals = j.entries("toolgate.refused");
        assert_eq!(refusals.len(), 1, "{named}: {refusals:#?}");
        let entry = &refusals[0];
        assert_eq!(
            entry["target"],
            format!("tool:table_write run:{} step:collect", outcome.run_id),
            "{named}"
        );
        let actor: Actor = serde_json::from_value(entry["actor"].clone())
            .unwrap_or_else(|e| panic!("{named}: the actor as JSON ({e}): {entry}"));
        assert!(
            matches!(&actor, Actor::Seat { seat, step, .. } if seat == "seat-collect" && step == "collect"),
            "{named}: {actor:?}"
        );
        assert!(
            j.items.iter().all(|i| i.kind != InboxKind::Failure),
            "{named}: {:#?}",
            j.items
        );
        let export = serde_json::to_string(&j.audit).unwrap();
        assert!(!export.contains(MADE_UP_TABLE), "{named}: never audited");
        assert!(
            !export.contains(&am_store::audit::sha256_hex(MADE_UP_TABLE)),
            "{named}: nor hashed"
        );
        assert!(rows_of(&p.db, TABLE).is_empty(), "{named}");
        assert!(rows_of(&p.db, OTHER_TABLE).is_empty(), "{named}");
    }
}

/// Row 7 (the verify): a step whose seat is in team `core` declares
/// `others`, a table of team `side`. Nothing reaches the table: either
/// `run_plan` refuses the plan before any run exists, naming the step and
/// the table, or the run offers no `table_write` and ToolGate refuses the
/// call (in words naming the table and both teams). No row in either
/// table, by either path.
#[tokio::test]
async fn another_teams_seat_is_refused() {
    let p = project();
    let llm = writes(&[insert_into(OTHER_TABLE, json!({ "ref": "r-1" }))]);
    let crossing = StepPlan {
        table: Some(others()),
        ..writing(step("collect", llm.clone()))
    };
    assert_eq!(crossing.seat_team, TEAM);

    let result = run_plan(
        &p.store,
        owner(),
        "manual",
        &plan(vec![crossing]),
        CancellationToken::new(),
    )
    .await;

    match result {
        Err(refused) => {
            let words = refused.to_string();
            assert!(words.contains("collect"), "names the step: {words}");
            assert!(words.contains(OTHER_TABLE), "names the table: {words}");
            assert!(
                Reader::open(&p.db).unwrap().runs().unwrap().is_empty(),
                "refused before any run row"
            );
            assert_eq!(llm.calls(), 0, "no model was called");
        }
        Ok(outcome) => {
            assert_eq!(outcome.status, "failed", "{outcome:?}");
            assert!(
                offered(&llm.requests()[0]).is_empty(),
                "another team's table is offered nothing"
            );
            let j = journal(&p.db);
            let attempt = j.attempt("collect", 1);
            assert!(
                j.tool_calls_of(&attempt.id)
                    .iter()
                    .all(|t| t.status == "refused"),
                "{:#?}",
                j.tool_calls_of(&attempt.id)
            );
            let words = outcome.reason.clone().unwrap_or_default();
            for word in [OTHER_TABLE, OTHER_TEAM, TEAM, "seat-collect"] {
                assert!(words.contains(word), "names `{word}`: {words}");
            }
        }
    }
    assert!(rows_of(&p.db, TABLE).is_empty());
    assert!(rows_of(&p.db, OTHER_TABLE).is_empty());
}

/// Row 12: a tainted step (an untrusted plan input) writes its declared
/// table: the row and its after-image carry taint 1. Its next call, to
/// `others`, is refused as row 6, and the refusal's audit args carry the
/// taint; `others` stays empty.
#[tokio::test]
async fn a_tainted_step_writes_only_its_declared_table() {
    let p = project();
    let llm = writes(&[
        insert(json!({ "ref": "t-1", "title": "from a tainted step" })),
        insert_into(OTHER_TABLE, json!({ "ref": "t-2" })),
    ]);

    let outcome = run(&p, vec![poisoned(writing(step("collect", llm.clone())))]).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let words = outcome.reason.clone().expect("in words");
    assert!(
        words.contains(TABLE) && words.contains("another table"),
        "{words}"
    );
    let row = only_row(&p.db, TABLE);
    assert_eq!(row.taint, Taint::Tainted, "{row:?}");
    let images = history_of(&p.db, TABLE, &row.row_id);
    assert_eq!(images.len(), 1, "{images:#?}");
    assert_eq!(images[0].taint, Taint::Tainted);
    assert!(rows_of(&p.db, OTHER_TABLE).is_empty());

    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);
    assert_eq!(attempt.taint, Taint::Tainted);
    assert_eq!(
        j.tool_calls_of(&attempt.id)
            .iter()
            .map(|t| t.status.as_str())
            .collect::<Vec<_>>(),
        vec!["succeeded", "refused"]
    );
    let refusals = j.entries("toolgate.refused");
    assert_eq!(refusals.len(), 1, "{refusals:#?}");
    assert!(
        audited_tainted(&refusals[0]["args"]["taint"]),
        "the refusal carries the step's taint: {}",
        refusals[0]
    );
}
