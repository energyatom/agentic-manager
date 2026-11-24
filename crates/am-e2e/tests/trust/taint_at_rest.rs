//! Trust test (AD-10 (c), AGENTS.md "Taint never launders"; epic 3 story 3,
//! matrix row 23): taint at rest only ever rises. `step_runs.taint` and an
//! Inbox item's `taint` go from 0 to 1 and never back: every writer command
//! that carries a taint, sent with 0 for a tainted attempt or item, is
//! refused or changes nothing, and a Review item never carries less taint
//! than its payload's producer. There is no command that clears taint, and
//! none may be added. Written before the taint columns exist.
#![forbid(unsafe_code)]

use am_domain::{Actor, InboxKind, Taint, Usage};
use am_store::{AttemptWrites, NewInboxItem, StepInput, Store, WriteCommand, new_id};

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

fn untrusted_input() -> StepInput {
    StepInput {
        kind: "untrusted".into(),
        from_step_run: None,
        from_item: None,
        label: "test:poisoned-page".into(),
        taint: Taint::Tainted,
    }
}

fn review(payload: &str, taint: Taint) -> NewInboxItem {
    NewInboxItem {
        id: new_id(),
        kind: InboxKind::Review,
        payload: Some(payload.into()),
        taint,
        title: "Review the version".into(),
        body: "The gate waits for your review of the previous step.".into(),
        max_returns: Some(2),
    }
}

/// A run of `steps` whose first step ran tainted and succeeded; returns the
/// store, the run id and the step-run ids in order.
fn tainted_first_step(dir: &std::path::Path, steps: &[&str]) -> (Store, String, Vec<String>) {
    let store = Store::open(&dir.join("state.db")).unwrap();
    let w = store.writer();
    let run = new_id();
    w.exec(
        engine(),
        WriteCommand::CreateRun {
            run_id: run.clone(),
            workflow: "wf".into(),
            origin: "manual".into(),
            steps: steps.iter().map(|s| s.to_string()).collect(),
            config_hash: None,
        },
    )
    .unwrap();
    let ids: Vec<String> = store
        .reader()
        .unwrap()
        .steps(&run)
        .unwrap()
        .into_iter()
        .map(|s| s.id)
        .collect();
    w.exec(
        engine(),
        WriteCommand::StartAttempt {
            step_run_id: ids[0].clone(),
            inputs: vec![untrusted_input()],
            taint: Taint::Tainted,
            prompt: vec![],
            writes: AttemptWrites {
                seat: "seat-draft".into(),
                table: None,
                document: None,
            },
        },
    )
    .unwrap();
    (store, run, ids)
}

fn taint_of(store: &Store, run: &str, id: &str) -> Taint {
    store
        .reader()
        .unwrap()
        .steps(run)
        .unwrap()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap()
        .taint
}

#[test]
fn no_command_clears_a_step_or_item_taint() {
    let dir = tempfile::tempdir().unwrap();
    let (store, run, ids) = tainted_first_step(dir.path(), &["draft", "approve", "publish"]);
    let w = store.writer();
    let (draft, approve) = (ids[0].clone(), ids[1].clone());
    assert_eq!(taint_of(&store, &run, &draft), Taint::Tainted);

    // A reservation that says clean leaves the attempt tainted.
    w.exec(
        engine(),
        WriteCommand::ReserveCall {
            step_run_id: draft.clone(),
            call_id: "call-1".into(),
            call_seq: 1,
            reserved_micro_usd: 0,
            rate_snapshot: "none".into(),
            taint: Taint::Clean,
            purpose: am_store::CallPurpose::Turn,
            prompt: vec![],
        },
    )
    .unwrap();
    assert_eq!(
        taint_of(&store, &run, &draft),
        Taint::Tainted,
        "after a clean reservation"
    );

    // Settling and finishing keep it.
    w.exec(
        engine(),
        WriteCommand::SettleAndFinishStep {
            call_id: "call-1".into(),
            usage: Some(Usage {
                input_tokens: 3,
                output_tokens: 2,
            }),
            cost_micro_usd: None,
            step_run_id: draft.clone(),
            status: "succeeded".into(),
            output: Some("a version that read a poisoned page".into()),
        },
    )
    .unwrap();
    assert_eq!(
        taint_of(&store, &run, &draft),
        Taint::Tainted,
        "after its finish"
    );

    // Starting it again as clean is refused (it has finished) and changes
    // nothing.
    let again = w.exec(
        engine(),
        WriteCommand::StartAttempt {
            step_run_id: draft.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
            writes: AttemptWrites {
                seat: "seat-draft".into(),
                table: None,
                document: None,
            },
        },
    );
    assert!(again.is_err(), "a finished attempt does not start again");
    assert_eq!(
        taint_of(&store, &run, &draft),
        Taint::Tainted,
        "after a clean start"
    );

    // The gate pauses with a tainted Review item; a second pause that says
    // clean changes nothing.
    w.exec(
        engine(),
        WriteCommand::PauseStep {
            step_run_id: approve.clone(),
            inputs: vec![StepInput {
                kind: "step_output".into(),
                from_step_run: Some(draft.clone()),
                from_item: None,
                label: "step:draft@1".into(),
                taint: Taint::Tainted,
            }],
            taint: Taint::Tainted,
            item: review(&draft, Taint::Tainted),
        },
    )
    .unwrap();
    let items = store.reader().unwrap().inbox_items(&run).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].taint, Taint::Tainted);
    let _ = w.exec(
        engine(),
        WriteCommand::PauseStep {
            step_run_id: approve.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            item: review(&draft, Taint::Clean),
        },
    );
    let now = store.reader().unwrap().inbox_items(&run).unwrap();
    assert_eq!(now, items, "the item is unchanged and still tainted");
    assert_eq!(taint_of(&store, &run, &approve), Taint::Tainted);
    assert_eq!(taint_of(&store, &run, &draft), Taint::Tainted);
    assert_eq!(store.reader().unwrap().verify_audit_chain().unwrap(), None);
}

/// A Review item never carries less taint than its payload's producer: a
/// pause that names a tainted payload but says clean is refused, or makes a
/// tainted item (the writer reads the producer's committed taint).
#[test]
fn a_review_item_is_never_cleaner_than_its_payload() {
    let dir = tempfile::tempdir().unwrap();
    let (store, run, ids) = tainted_first_step(dir.path(), &["draft", "approve"]);
    let w = store.writer();
    w.exec(
        engine(),
        WriteCommand::FinishStep {
            step_run_id: ids[0].clone(),
            status: "succeeded".into(),
            output: Some("a version that read a poisoned page".into()),
        },
    )
    .unwrap();

    let said_clean = w.exec(
        engine(),
        WriteCommand::PauseStep {
            step_run_id: ids[1].clone(),
            inputs: vec![],
            taint: Taint::Clean,
            item: review(&ids[0], Taint::Clean),
        },
    );

    let items = store.reader().unwrap().inbox_items(&run).unwrap();
    match said_clean {
        Ok(()) => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].taint, Taint::Tainted, "copied from its payload");
        }
        Err(_) => assert!(items.is_empty(), "refused: {items:#?}"),
    }
}
