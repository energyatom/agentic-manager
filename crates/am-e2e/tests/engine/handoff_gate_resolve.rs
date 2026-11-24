//! Epic 3 story 4, the engine's side of the Owner gate (AD-8, AD-10,
//! AD-15, AD-16, AD-27, AD-28, AD-36, AD-43; matrix rows 2 to 7, 11, 29 to
//! 31 and 33): a person resolves a waiting gate's Review item
//! (`Writer::resolve_inbox_item`, `HumanActor`), and the engine applies the
//! committed decision as `System(engine)` (`am_engine::gate::
//! apply_resolution`) and goes on (`am_engine::run::continue_run`).
//!
//! - approve: the gate succeeds and the step after it receives the
//!   reviewed payload (the producer's committed output and taint);
//! - return: the gate attempt is `looped_back`, the producer and the gate
//!   get their next attempt, and the producer's new attempt has one
//!   `gate_note` input from the item, whose note reaches its prompt as a
//!   trusted part (and nothing of its own earlier output); past the Review
//!   item's `max_returns`, read from the journal, the gate fails with one
//!   Failure item and the run fails;
//! - reject: the gate and the run fail, with no Failure item;
//! - `continue_run` applies a committed resolution once, refuses another
//!   snapshot than the run's pin (`RunError::ConfigChanged`) and a step
//!   left running (`RunError::NotContinuable`, resume is story 3.9);
//! - an API resolve (3.2's server, holding the project's writer) records
//!   and applies the decision and runs no model step; `continue_run` then
//!   finishes the run.
//!
//! FakeLlm only; no network beyond 127.0.0.1. Written before the resolve,
//! the apply or `continue_run` exist.
#![forbid(unsafe_code)]

#[path = "support/mod.rs"]
mod support;

#[path = "../trust/support/server.rs"]
mod server;

#[path = "../trust/support/inbox.rs"]
mod inbox;

use std::sync::Arc;

use am_domain::{Decision, HumanActor, InboxKind, LlmProvider, Part, REVIEWER_HAT, Taint};
use am_engine::run::{HandOff, RunError, RunOutcome, RunPlan, StepPlan, continue_run, run_plan};
use am_store::{Reader, StepInput, Store, WriteCommand, new_id};
use support::*;
use tokio_util::sync::CancellationToken;

const DRAFT_1: &str = "the first version";
const DRAFT_2: &str = "the second version";
const NOTE: &str = "tighten the summary";
/// Another snapshot's hash.
const OTHER: &str = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";

fn reviewer() -> HumanActor {
    HumanActor::new("owner", REVIEWER_HAT)
}

/// [`step`] as a human gate that takes at most `max` returns.
fn gate_max(key: &str, provider: Arc<dyn LlmProvider>, max: u32) -> StepPlan {
    StepPlan {
        hand_off: HandOff::Gate { max_returns: max },
        ..step(key, provider)
    }
}

/// The fixture's shape: `draft`, the gate `approve`, `publish`.
fn gated(draft: Arc<dyn LlmProvider>, publish: Arc<dyn LlmProvider>, max: u32) -> RunPlan {
    plan(vec![
        step("draft", draft),
        gate_max("approve", never(), max),
        step("publish", publish),
    ])
}

/// Starts a run of `plan`, which waits at its gate.
async fn start(store: &Store, plan: &RunPlan) -> RunOutcome {
    let outcome = run_plan(store, owner(), "manual", plan, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    assert!(outcome.item.is_some(), "{outcome:?}");
    outcome
}

async fn go_on(store: &Store, run_id: &str, plan: &RunPlan) -> Result<RunOutcome, RunError> {
    continue_run(store, run_id, plan, CancellationToken::new()).await
}

fn resolve(store: &Store, item: &str, decision: Decision, note: Option<&str>) {
    store
        .writer()
        .resolve_inbox_item(reviewer(), item, decision, note.map(str::to_string))
        .unwrap_or_else(|e| panic!("{decision:?} on {item}: {e}"));
}

/// `(key, attempt, status)` rows, as [`Journal::shape`] gives them.
fn shape(rows: &[(&str, i64, &str)]) -> Vec<(String, i64, String)> {
    rows.iter()
        .map(|(k, a, s)| (k.to_string(), *a, s.to_string()))
        .collect()
}

/// Every audit entry after the first `inbox.resolved` one is the
/// engine's: the person decided, the engine applied (AD-15).
fn follow_ups_are_the_engines(j: &Journal) -> Vec<String> {
    let at = j
        .audit
        .iter()
        .position(|e| e["action"] == "inbox.resolved")
        .unwrap_or_else(|| panic!("no inbox.resolved entry: {:#?}", j.audit));
    let resolved = &j.audit[at];
    assert_eq!(resolved["actor"]["kind"], "human", "{resolved}");
    assert_eq!(resolved["actor"]["hat"], "Reviewer", "{resolved}");
    let after: Vec<&serde_json::Value> = j.audit[at + 1..].iter().collect();
    for e in &after {
        if e["action"] == "inbox.resolved" {
            continue;
        }
        assert_eq!(e["actor"]["kind"], "system", "{e}");
        assert_eq!(e["actor"]["component"], "engine", "{e}");
    }
    after
        .iter()
        .map(|e| e["action"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// Rows 2 and 29: approve, then `continue_run`: the gate attempt succeeds
/// with no model call, `publish` runs on its model receiving `draft`'s
/// committed output (the Review's payload) as the trusted input
/// `step:draft@1` (one `step_output` input from `draft`'s attempt, its
/// taint as committed), and the run succeeds. The producer does not run
/// again; no other item is made; the apply's entries are the engine's.
#[tokio::test]
async fn approve_continues_the_run_and_the_next_step_gets_the_reviewed_output() {
    let p = project();
    let (draft, after) = (texts(&[DRAFT_1]), texts(&["prepared"]));
    let plan = gated(draft.clone(), after.clone(), 2);
    let waiting = start(&p.store, &plan).await;
    let item = waiting.item.clone().unwrap();
    resolve(&p.store, &item, Decision::Approve, None);

    let outcome = go_on(&p.store, &waiting.run_id, &plan).await.unwrap();

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert_eq!(outcome.run_id, waiting.run_id);
    assert_eq!(draft.calls(), 1, "the producer does not run again");
    assert_eq!(after.calls(), 1);
    let j = journal(&p.db);
    assert_eq!(
        j.shape(),
        shape(&[
            ("draft", 1, "succeeded"),
            ("approve", 1, "succeeded"),
            ("publish", 1, "succeeded"),
        ])
    );
    assert_eq!(j.runs[0].status, "succeeded");
    let d = j.attempt("draft", 1);
    let publish = j.attempt("publish", 1);
    let parts = user_parts(&after.requests()[0]);
    assert_eq!(trusted(&parts, "step:draft@1").text(), DRAFT_1);
    assert_eq!(
        j.inputs_of(&publish.id),
        vec![StepInput {
            kind: "step_output".into(),
            from_step_run: Some(d.id.clone()),
            from_item: None,
            label: "step:draft@1".into(),
            taint: Taint::Clean,
        }]
    );
    assert_eq!(publish.taint, Taint::Clean);
    assert_eq!(j.items.len(), 1, "{:#?}", j.items);
    let review = &j.items[0];
    assert_eq!(review.id, item);
    assert_eq!(review.payload.as_deref(), Some(d.id.as_str()));
    assert_eq!(review.decision, Some(Decision::Approve));
    let applied = follow_ups_are_the_engines(&j);
    assert!(
        applied.iter().any(|a| a.contains("step.finished")),
        "{applied:?}"
    );
    assert_eq!(
        p.store.reader().unwrap().verify_audit_chain().unwrap(),
        None
    );
}

/// Row 3: a return, applied: the gate attempt 1 is `looped_back`; attempt 2
/// of `draft` and of `approve` are queued (not of `publish`); `draft`'s
/// attempt 2 has exactly one input, `gate_note` from the item (no producing
/// step run, labelled `gate-note:approve@1`, taint 0), and the gate's
/// attempt 2 none yet; the note is on the item only. Going on, `draft`
/// runs again and the gate waits again with a new open Review item (its
/// `max_returns` 2, its payload `draft`'s attempt 2).
#[tokio::test]
async fn return_queues_the_producer_and_the_gate_with_one_gate_note_input() {
    let p = project();
    let draft = texts(&[DRAFT_1, DRAFT_2]);
    let plan = gated(draft.clone(), never(), 2);
    let waiting = start(&p.store, &plan).await;
    let item = waiting.item.clone().unwrap();
    resolve(&p.store, &item, Decision::Return, Some(NOTE));

    {
        let reader = p.store.reader().unwrap();
        am_engine::gate::apply_resolution(&p.store.writer(), &reader, &item)
            .expect("the return is applied");
    }

    let j = journal(&p.db);
    assert_eq!(
        j.shape(),
        shape(&[
            ("draft", 1, "succeeded"),
            ("draft", 2, "queued"),
            ("approve", 1, "looped_back"),
            ("approve", 2, "queued"),
            ("publish", 1, "queued"),
        ])
    );
    assert_eq!(j.runs[0].status, "running");
    let redraft = j.attempt("draft", 2);
    assert_eq!(
        j.inputs_of(&redraft.id),
        vec![StepInput {
            kind: "gate_note".into(),
            from_step_run: None,
            from_item: Some(item.clone()),
            label: "gate-note:approve@1".into(),
            taint: Taint::Clean,
        }]
    );
    assert!(j.inputs_of(&j.attempt("approve", 2).id).is_empty());
    let review = j.items.iter().find(|i| i.id == item).unwrap();
    assert_eq!(review.note.as_deref(), Some(NOTE));
    assert_eq!(review.decision, Some(Decision::Return));
    for inputs in j.inputs.values() {
        for input in inputs {
            assert!(!input.label.contains(NOTE), "{input:?}");
        }
    }
    let applied = follow_ups_are_the_engines(&j);
    assert!(
        applied.iter().any(|a| a.contains("looped_back")),
        "{applied:?}"
    );

    let outcome = go_on(&p.store, &waiting.run_id, &plan).await.unwrap();

    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    assert_eq!(draft.calls(), 2);
    let j = journal(&p.db);
    assert_eq!(j.attempt("draft", 2).status, "succeeded");
    assert_eq!(j.attempt("approve", 2).status, "waiting_human");
    let again = outcome.item.clone().expect("a new Review item");
    assert_ne!(again, item);
    let new = j.items.iter().find(|i| i.id == again).unwrap();
    assert_eq!(new.kind, InboxKind::Review);
    assert_eq!(new.status, "open");
    assert_eq!(new.max_returns, Some(2));
    assert_eq!(new.step_run_id, j.attempt("approve", 2).id);
    assert_eq!(
        new.payload.as_deref(),
        Some(j.attempt("draft", 2).id.as_str())
    );
    assert_eq!(j.items.len(), 2);
}

/// Row 3 (AD-10 (b), AD-27): the producer's new request holds the note as
/// its own `Part::Trusted`, labelled `gate-note:approve@1`, in the user
/// message after the step's instruction and before any fenced part, once,
/// and never in the system position. The producer receives the note and
/// its usual inputs, not its own earlier output (AD-36 names only the
/// note).
#[tokio::test]
async fn the_note_reaches_the_producers_prompt_as_a_trusted_part() {
    let p = project();
    let draft = texts(&[DRAFT_1, DRAFT_2]);
    let plan = gated(draft.clone(), never(), 2);
    let waiting = start(&p.store, &plan).await;
    resolve(
        &p.store,
        waiting.item.as_deref().unwrap(),
        Decision::Return,
        Some(NOTE),
    );

    let outcome = go_on(&p.store, &waiting.run_id, &plan).await.unwrap();

    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    let requests = draft.requests();
    assert_eq!(requests.len(), 2);
    let request = &requests[1];
    let parts = user_parts(request);
    assert!(
        matches!(&parts[0], Part::Instruction(i) if i.text() == instruction_of("draft")),
        "{parts:#?}"
    );
    assert_eq!(trusted(&parts, "gate-note:approve@1").text(), NOTE);
    let at = parts
        .iter()
        .position(|part| matches!(part, Part::Trusted(t) if t.label() == "gate-note:approve@1"))
        .unwrap();
    assert!(at > 0, "after the instruction");
    assert!(
        !parts[..at]
            .iter()
            .any(|part| matches!(part, Part::Untrusted(_))),
        "before the fence: {parts:#?}"
    );
    let carrying = carriers(request, NOTE);
    assert_eq!(carrying.len(), 1, "once: {carrying:#?}");
    assert!(matches!(&carrying[0], Part::Trusted(_)), "{carrying:#?}");
    assert!(!request.system.iter().any(|i| i.text().contains(NOTE)));
    assert!(
        carriers(request, DRAFT_1).is_empty(),
        "not its own earlier output: {request:#?}"
    );
}

/// Row 4: the return count is the journal's. With `max_returns` 2 and the
/// store reopened (a fresh process) before each `continue_run`, returns 1
/// and 2 rerun `draft` and the gate waits again; a fresh `Reader` counts
/// the resolved returns and reads each new Review item's `max_returns`;
/// the third return fails the gate.
#[tokio::test]
async fn the_return_count_comes_from_the_journal() {
    let p = project();
    let plan = gated(texts(&["v1", "v2", "v3", "v4"]), never(), 2);
    let waiting = start(&p.store, &plan).await;
    let run = waiting.run_id.clone();
    let mut item = waiting.item.clone().unwrap();
    let Project { dir, db, store } = p;
    let mut store = store;
    for n in 1..=3usize {
        resolve(
            &store,
            &item,
            Decision::Return,
            Some(format!("note {n}").as_str()),
        );
        drop(store);
        store = Store::open(&db).unwrap();

        let outcome = go_on(&store, &run, &plan).await.unwrap();

        let fresh = Reader::open(&db).unwrap();
        let returns = fresh
            .inbox_items(&run)
            .unwrap()
            .iter()
            .filter(|i| i.decision == Some(Decision::Return))
            .count();
        assert_eq!(returns, n);
        if n < 3 {
            assert_eq!(outcome.status, "waiting_human", "return {n}: {outcome:?}");
            item = outcome.item.clone().unwrap();
            let next = fresh.inbox_item(&item).unwrap().unwrap();
            assert_eq!((next.status.as_str(), next.max_returns), ("open", Some(2)));
        } else {
            assert_eq!(outcome.status, "failed", "return 3 of 2: {outcome:?}");
        }
    }
    let j = journal(&db);
    assert_eq!(j.attempts("draft").len(), 3);
    assert_eq!(j.attempt("approve", 3).status, "failed");
    drop(store);
    drop(dir);
}

/// Rows 4 and 5 (AD-36): with `max_returns` 2, returns 1 and 2 rerun the
/// producer; the third fails the gate attempt 3 with one Failure item on
/// it (the engine's words name the step, `max_returns`, 2 and the Review
/// item's id; never a note), the run fails and the outcome names the item;
/// the third note reaches no prompt and stays on its item. With
/// `max_returns` 0 the first return does the same.
#[tokio::test]
async fn past_max_returns_the_gate_fails_with_one_failure_item() {
    let p = project();
    let draft = texts(&["v1", "v2", "v3", "v4"]);
    let plan = gated(draft.clone(), never(), 2);
    let waiting = start(&p.store, &plan).await;
    let mut item = waiting.item.clone().unwrap();
    let notes = ["first note", "second note", "third note"];
    let mut last = None;
    for (n, note) in notes.iter().enumerate() {
        resolve(&p.store, &item, Decision::Return, Some(*note));
        let outcome = go_on(&p.store, &waiting.run_id, &plan).await.unwrap();
        if n < 2 {
            assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
            item = outcome.item.clone().unwrap();
        } else {
            last = Some(outcome);
        }
    }
    let outcome = last.unwrap();

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    assert!(outcome.reason.is_some(), "{outcome:?}");
    let j = journal(&p.db);
    let gate = j.attempt("approve", 3);
    assert_eq!(gate.status, "failed");
    let failures: Vec<_> = j
        .items
        .iter()
        .filter(|i| i.kind == InboxKind::Failure)
        .collect();
    assert_eq!(failures.len(), 1, "{:#?}", j.items);
    let failure = failures[0];
    assert_eq!(failure.step_run_id, gate.id);
    assert_eq!(failure.status, "open");
    assert_eq!(outcome.item.as_deref(), Some(failure.id.as_str()));
    for word in ["approve", "max_returns"] {
        assert!(failure.body.contains(word), "{word}: {}", failure.body);
    }
    assert!(names_number(&failure.body, 2), "{}", failure.body);
    assert!(
        failure.body.contains(&item),
        "names the Review item {item}: {}",
        failure.body
    );
    for note in notes {
        assert!(!failure.title.contains(note) && !failure.body.contains(note));
    }
    assert_eq!(j.runs[0].status, "failed");
    assert_eq!(j.attempts("draft").len(), 3);
    assert_eq!(draft.calls(), 3, "returns 1 and 2 rerun the producer");
    for request in draft.requests() {
        assert!(carriers(&request, "third note").is_empty());
    }
    let third = j.items.iter().find(|i| i.id == item).unwrap();
    assert_eq!(third.note.as_deref(), Some("third note"));
    assert_eq!(j.attempt("publish", 1).status, "queued");

    // max_returns 0.
    let p = project();
    let draft = texts(&["v1"]);
    let plan = gated(draft.clone(), never(), 0);
    let waiting = start(&p.store, &plan).await;
    let item = waiting.item.clone().unwrap();
    resolve(&p.store, &item, Decision::Return, Some("the only note"));
    let outcome = go_on(&p.store, &waiting.run_id, &plan).await.unwrap();
    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let j = journal(&p.db);
    assert_eq!(draft.calls(), 1);
    assert_eq!(j.attempts("draft").len(), 1);
    assert_eq!(j.attempt("approve", 1).status, "failed");
    let review = j.items.iter().find(|i| i.id == item).unwrap();
    assert_eq!(review.max_returns, Some(0));
    assert_eq!(
        j.items
            .iter()
            .filter(|i| i.kind == InboxKind::Failure)
            .count(),
        1
    );
}

/// Row 7 (Question 7): reject, then `continue_run`: the gate attempt and
/// the run fail, the reason says it was rejected at step `approve` by You
/// (Reviewer), `publish` stays queued and runs no model, and no Failure
/// item is made (the Review item records the decision).
#[tokio::test]
async fn reject_fails_the_gate_and_the_run() {
    let p = project();
    let after = never();
    let plan = gated(texts(&[DRAFT_1]), after.clone(), 2);
    let waiting = start(&p.store, &plan).await;
    let item = waiting.item.clone().unwrap();
    resolve(&p.store, &item, Decision::Reject, None);

    let outcome = go_on(&p.store, &waiting.run_id, &plan).await.unwrap();

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let reason = outcome.reason.clone().expect("the reason");
    for word in ["rejected", "approve", "You (Reviewer)"] {
        assert!(reason.contains(word), "{word}: {reason}");
    }
    let j = journal(&p.db);
    assert_eq!(
        j.shape(),
        shape(&[
            ("draft", 1, "succeeded"),
            ("approve", 1, "failed"),
            ("publish", 1, "queued"),
        ])
    );
    assert_eq!(j.runs[0].status, "failed");
    assert!(j.runs[0].finished_at.is_some());
    assert_eq!(after.calls(), 0);
    assert_eq!(j.items.len(), 1, "no Failure item: {:#?}", j.items);
    assert_eq!(j.items[0].decision, Some(Decision::Reject));
    let applied = follow_ups_are_the_engines(&j);
    assert!(
        applied.iter().any(|a| a.contains("run.finished")),
        "{applied:?}"
    );
}

/// Row 30: the resolve committed and the process stopped before the apply
/// (the gate `running`, its item resolved). The store is reopened and
/// `continue_run` applies the decision once and goes on (approve: the run
/// succeeds; return: the gate waits again). A second apply of the item is
/// refused and changes nothing: no duplicate attempt, input or item.
#[tokio::test]
async fn a_resolve_stopped_before_its_apply_is_applied_once() {
    for (decision, note) in [(Decision::Approve, None), (Decision::Return, Some(NOTE))] {
        let p = project();
        let plan = gated(texts(&[DRAFT_1, DRAFT_2]), texts(&["prepared"]), 2);
        let waiting = start(&p.store, &plan).await;
        let item = waiting.item.clone().unwrap();
        resolve(&p.store, &item, decision, note);
        let Project { dir, db, store } = p;
        drop(store);
        let stopped = journal(&db);
        assert_eq!(stopped.attempt("approve", 1).status, "running");
        assert_eq!(stopped.items[0].status, "resolved");

        let store = Store::open(&db).unwrap();
        let outcome = go_on(&store, &waiting.run_id, &plan).await.unwrap();

        let j = journal(&db);
        if decision == Decision::Approve {
            assert_eq!(outcome.status, "succeeded", "{outcome:?}");
            assert_eq!(
                j.shape(),
                shape(&[
                    ("draft", 1, "succeeded"),
                    ("approve", 1, "succeeded"),
                    ("publish", 1, "succeeded"),
                ])
            );
        } else {
            assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
            assert_eq!(
                j.shape(),
                shape(&[
                    ("draft", 1, "succeeded"),
                    ("draft", 2, "succeeded"),
                    ("approve", 1, "looped_back"),
                    ("approve", 2, "waiting_human"),
                    ("publish", 1, "queued"),
                ])
            );
            let notes: usize = j
                .inputs
                .values()
                .flatten()
                .filter(|i| i.kind == "gate_note")
                .count();
            assert_eq!(notes, 1, "one gate_note input");
        }
        {
            let reader = store.reader().unwrap();
            assert!(
                am_engine::gate::apply_resolution(&store.writer(), &reader, &item).is_err(),
                "{decision:?}: applied once"
            );
        }
        let after = journal(&db);
        assert_eq!(after.steps, j.steps);
        assert_eq!(after.items, j.items);
        assert_eq!(after.inputs, j.inputs);
        drop(store);
        drop(dir);
    }
}

/// Row 31 (Question 2, AD-16): `continue_run` with a plan built from
/// another snapshot than the run's pin is refused with
/// `RunError::ConfigChanged { pinned, now }`, in words naming both hashes,
/// before anything is written: the resolution stays unapplied and no model
/// is called.
#[tokio::test]
async fn continue_refuses_another_snapshot() {
    let p = project();
    let after = texts(&["prepared"]);
    let plan = gated(texts(&[DRAFT_1]), after.clone(), 2);
    let waiting = start(&p.store, &plan).await;
    resolve(
        &p.store,
        waiting.item.as_deref().unwrap(),
        Decision::Approve,
        None,
    );
    let before = journal(&p.db);
    let changed = RunPlan {
        config_hash: OTHER.into(),
        ..plan.clone()
    };

    let refused = go_on(&p.store, &waiting.run_id, &changed).await;

    match refused {
        Err(e @ RunError::ConfigChanged { .. }) => {
            let words = e.to_string();
            for hash in [HASH, OTHER] {
                assert!(words.contains(&hash[..12]), "names {hash}: {words}");
            }
            let RunError::ConfigChanged { pinned, now } = e else {
                unreachable!()
            };
            assert_eq!((pinned.as_str(), now.as_str()), (HASH, OTHER));
        }
        other => panic!("refused for the changed snapshot: {other:?}"),
    }
    let now = journal(&p.db);
    assert_eq!(now.steps, before.steps);
    assert_eq!(now.items, before.items);
    assert_eq!(now.inputs, before.inputs);
    assert_eq!(now.calls, before.calls);
    assert_eq!(after.calls(), 0);
}

/// The boundary with story 3.9: `continue_run` takes only a clean stop. A
/// run with a step left `running` (an unsettled call) is refused with
/// `RunError::NotContinuable`, in words naming story 3.9, and nothing
/// changes; so is a finished run, and an unknown run id is an error.
#[tokio::test]
async fn continue_refuses_a_step_left_running() {
    let p = project();
    let model = texts(&[DRAFT_1]);
    let plan = gated(model.clone(), never(), 2);
    let run = new_id();
    let w = p.store.writer();
    w.exec(
        engine(),
        WriteCommand::CreateRun {
            run_id: run.clone(),
            workflow: "wf".into(),
            origin: "manual".into(),
            steps: vec!["draft".into(), "approve".into(), "publish".into()],
            config_hash: Some(HASH.into()),
        },
    )
    .unwrap();
    let draft = Reader::open(&p.db).unwrap().steps(&run).unwrap()[0]
        .id
        .clone();
    w.exec(
        engine(),
        WriteCommand::StartAttempt {
            step_run_id: draft.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
            writes: am_store::AttemptWrites {
                seat: "seat-draft".into(),
                table: None,
                document: None,
            },
        },
    )
    .unwrap();
    w.exec(
        engine(),
        WriteCommand::ReserveCall {
            step_run_id: draft,
            call_id: new_id(),
            call_seq: 1,
            reserved_micro_usd: 0,
            rate_snapshot: "none".into(),
            taint: Taint::Clean,
            purpose: am_store::CallPurpose::Turn,
            prompt: vec![],
        },
    )
    .unwrap();
    let before = journal(&p.db);

    match go_on(&p.store, &run, &plan).await {
        Err(RunError::NotContinuable(words)) => {
            assert!(words.contains("3.9"), "names story 3.9: {words}");
        }
        other => panic!("a step left running is not continued: {other:?}"),
    }
    let now = journal(&p.db);
    assert_eq!(now.steps, before.steps);
    assert_eq!(now.calls, before.calls);
    assert_eq!(model.calls(), 0);

    // A finished run.
    let one = plan_of_one();
    let done = run_plan(&p.store, owner(), "manual", &one, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(done.status, "succeeded");
    assert!(matches!(
        go_on(&p.store, &done.run_id, &one).await,
        Err(RunError::NotContinuable(_))
    ));
    // An unknown run.
    assert!(go_on(&p.store, &new_id(), &plan).await.is_err());
}

/// A one-step plan on the pinned hash.
fn plan_of_one() -> RunPlan {
    plan(vec![step("only", texts(&["done"]))])
}

/// Rows 11 and 33 (Question 1): on 3.2's server holding the project's
/// writer, the same approve over `/api/v1` (session, CSRF, Origin) is 200:
/// the item is resolved and the server applies it as `System(engine)` (the
/// gate `succeeded`) but runs no model step, so the run is left ready
/// (`running`, `publish` queued). `continue_run` then finishes it, and
/// `publish` receives the reviewed output.
#[tokio::test]
async fn an_api_resolve_leaves_the_run_ready_and_continue_finishes_it() {
    let rig = inbox::with_project_writer().await;
    let store = rig.store.as_ref().expect("a project");
    let db = rig.runtime.as_ref().unwrap().state_db();
    let after = texts(&["prepared"]);
    let plan = gated(texts(&[DRAFT_1]), after.clone(), 2);
    let waiting = start(store, &plan).await;
    let item = waiting.item.clone().unwrap();
    let s = rig.sign_in().await;

    let r = inbox::resolve(&rig, &s, &item, r#"{"decision":"approve"}"#).await;

    assert_eq!(r.status, 200, "{}", r.text());
    let j = journal(&db);
    assert_eq!(
        j.shape(),
        shape(&[
            ("draft", 1, "succeeded"),
            ("approve", 1, "succeeded"),
            ("publish", 1, "queued"),
        ])
    );
    assert_eq!(j.runs[0].status, "running", "left ready");
    assert_eq!(after.calls(), 0, "the server runs no model step");
    let review = j.items.iter().find(|i| i.id == item).unwrap();
    assert_eq!(review.decision, Some(Decision::Approve));
    follow_ups_are_the_engines(&j);

    let outcome = go_on(store, &waiting.run_id, &plan).await.unwrap();

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert_eq!(after.calls(), 1);
    let parts = user_parts(&after.requests()[0]);
    assert_eq!(trusted(&parts, "step:draft@1").text(), DRAFT_1);
    assert_eq!(journal(&db).runs[0].status, "succeeded");
}

// ------------------------------------------------------- review round 1
//
// Story 3.4 review round 1 (the coordinator's triage, 2026-11-24), R6:
// `continue_run` checks that the plan is the run's workflow. Written
// before the fix.

/// R6: `continue_run` with a plan for another workflow of the same snapshot
/// (the same pinned hash and step keys) is refused `NotContinuable` before
/// anything is applied: the resolved Review stays as it was, unapplied, and
/// the gate stays `running`; no model is called.
#[tokio::test]
async fn continue_refuses_a_plan_for_another_workflow() {
    let p = project();
    let after = texts(&["prepared"]);
    let plan = gated(texts(&[DRAFT_1]), after.clone(), 2);
    let waiting = start(&p.store, &plan).await;
    let item = waiting.item.clone().unwrap();
    resolve(&p.store, &item, Decision::Approve, None);
    let before = journal(&p.db);
    let another = RunPlan {
        workflow: "another-workflow".into(),
        ..plan.clone()
    };
    assert_eq!(
        another.config_hash,
        before.runs[0].config_hash.clone().unwrap()
    );

    let refused = go_on(&p.store, &waiting.run_id, &another).await;

    assert!(
        matches!(refused, Err(RunError::NotContinuable(_))),
        "{refused:?}"
    );
    let now = journal(&p.db);
    assert_eq!(now.steps, before.steps);
    assert_eq!(now.items, before.items);
    assert_eq!(now.inputs, before.inputs);
    assert_eq!(now.attempt("approve", 1).status, "running");
    let review = now.items.iter().find(|i| i.id == item).unwrap();
    assert_eq!(review.decision, Some(Decision::Approve));
    assert_eq!(after.calls(), 0);
}
