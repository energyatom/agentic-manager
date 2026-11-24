//! The spike-scope journaled executor (`am_engine::journal`): a run settles
//! from usage and a cancel settles at the reservation. Moved here from the
//! module's unit tests in story 3.5 (review T1): the module names no
//! `Instruction::authored`, so a test that needs an `Instruction` lives
//! outside it.
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use am_domain::{Actor, Instruction};
use am_engine::journal::{LinearWorkflow, Price, create_run, drive_run};
use am_testkit::{FakeLlm, ScriptedTurn};
use tokio_util::sync::CancellationToken;

fn system(component: &str) -> Actor {
    Actor::System {
        component: component.into(),
    }
}

fn step_prompt(step_key: &str, workflow: &str) -> Instruction {
    Instruction::authored(format!(
        "Step {step_key} of workflow {workflow}. Reply with one short line."
    ))
}

fn wf() -> LinearWorkflow {
    LinearWorkflow {
        workflow: "wf".into(),
        steps: vec!["a".into(), "b".into()],
        model: "fake".into(),
        max_output_tokens: 100,
        price: Price {
            input_micro_per_mtok: 3_000_000,
            output_micro_per_mtok: 15_000_000,
            snapshot: "test".into(),
        },
        system: Instruction::authored("You are a step in a test workflow."),
        step_prompt,
    }
}

#[tokio::test]
async fn runs_to_success_and_settles_from_usage() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = am_store::Store::open(&db).unwrap();
    let w = store.writer();
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("ok")
            .usage(1000, 100)
            .delay(Duration::from_millis(1)),
    ]));
    let run = create_run(&w, system("test"), &wf(), "manual").unwrap();
    let status = drive_run(&db, &w, llm.clone(), &wf(), &run, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(status, "succeeded");
    assert_eq!(llm.calls(), 2);
    let l = store.reader().unwrap().ledger_for_run(&run).unwrap();
    assert!(l.iter().all(|e| e.settled_micro_usd == Some(4500)));
}

#[tokio::test]
async fn cancel_settles_at_reservation_when_usage_missing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = am_store::Store::open(&db).unwrap();
    let w = store.writer();
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("slow").delay(Duration::from_secs(30)),
    ]));
    let run = create_run(&w, system("test"), &wf(), "manual").unwrap();
    let token = CancellationToken::new();
    let t2 = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        t2.cancel();
    });
    let status = drive_run(&db, &w, llm, &wf(), &run, token).await.unwrap();
    assert_eq!(status, "cancelled");
    let l = store.reader().unwrap().ledger_for_run(&run).unwrap();
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].settled_micro_usd, Some(l[0].reserved_micro_usd));
}
