//! Epic 3 story 3, the human gate (AD-8, AD-15, AD-28, AD-36, AD-42;
//! matrix rows 9 to 12, 27 and 32): a step declared `gate: review` makes no
//! model call; it moves from `queued` to `waiting_human` together with one
//! Review Inbox item (payload: the previous step's committed attempt, by
//! reference), in one transaction, created by `System(engine)` only. The
//! run stays `running`; later steps stay `queued`; no model output, no
//! restart, no second pause and no config edit moves the gate on: only a
//! Human resolve (story 3.4) will. Written before the gate exists.
#![forbid(unsafe_code)]

#[path = "support/mod.rs"]
mod support;

#[path = "../trust/support/server.rs"]
mod server;

use std::time::Duration;

use am_domain::{InboxKind, Taint};
use am_store::{NewInboxItem, StepInput, Store, WriteCommand, new_id};
use support::*;
use tokio_util::sync::CancellationToken;

async fn run_on(store: &Store, plan: &am_engine::run::RunPlan) -> am_engine::run::RunOutcome {
    am_engine::run::run_plan(store, owner(), "manual", plan, CancellationToken::new())
        .await
        .unwrap()
}

/// The fixture's shape: a step, a gate, a step after it.
fn gated(
    first: std::sync::Arc<dyn am_domain::LlmProvider>,
    gate_model: std::sync::Arc<dyn am_domain::LlmProvider>,
    after: std::sync::Arc<dyn am_domain::LlmProvider>,
) -> am_engine::run::RunPlan {
    plan(vec![
        step("draft", first),
        gate("approve", gate_model),
        step("publish", after),
    ])
}

/// Row 9: step 1 succeeds; the gate step waits with one open Review item
/// whose payload is step 1's attempt; no model call for the gate step; the
/// step after it stays queued; the run stays `running` and the outcome says
/// `waiting_human`. The item is the engine's, audited `inbox.created`
/// (kind, step run, taint; never its body), and holds no model text.
#[tokio::test]
async fn a_gate_waits_with_one_review_item_and_no_model_call() {
    let p = project();
    let (first, gate_model, after) = (texts(&["the version to approve"]), never(), never());
    let plan = gated(first.clone(), gate_model.clone(), after.clone());

    let outcome = run_on(&p.store, &plan).await;

    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    assert_eq!(outcome.reason, None);
    assert_eq!(first.calls(), 1);
    assert_eq!(gate_model.calls(), 0, "a gate makes no model call");
    assert_eq!(after.calls(), 0, "nothing passes a waiting gate");
    let j = journal(&p.db);
    assert_eq!(
        j.shape(),
        vec![
            ("draft".to_string(), 1, "succeeded".to_string()),
            ("approve".to_string(), 1, "waiting_human".to_string()),
            ("publish".to_string(), 1, "queued".to_string()),
        ]
    );
    let (draft, approve) = (j.attempt("draft", 1), j.attempt("approve", 1));
    assert_eq!(approve.finished_at, None);
    assert_eq!(j.calls.len(), 1, "one model call, the first step's");
    assert_eq!(j.runs[0].status, "running");
    assert_eq!(j.runs[0].finished_at, None);

    assert_eq!(j.items.len(), 1, "{:#?}", j.items);
    let item = &j.items[0];
    assert_eq!(item.kind, InboxKind::Review);
    assert_eq!(item.status, "open");
    assert_eq!(item.run_id, j.runs[0].id);
    assert_eq!(item.step_run_id, approve.id);
    assert_eq!(item.payload.as_deref(), Some(draft.id.as_str()));
    assert_eq!(item.taint, Taint::Clean);
    assert!(!item.title.trim().is_empty() && !item.body.trim().is_empty());
    for text in [&item.title, &item.body] {
        assert!(
            !text.contains("the version to approve"),
            "an item's words are the engine's, never model text: {text}"
        );
    }
    assert_eq!(
        (item.resolved_at.clone(), item.resolved_by.clone()),
        (None, None)
    );
    let created = j.entries("inbox.created");
    assert_eq!(created.len(), 1, "{:#?}", j.audit);
    let e = &created[0];
    assert_eq!(e["target"], item.id.as_str());
    assert_eq!(e["actor"]["kind"], "system");
    assert_eq!(e["actor"]["component"], "engine");
    assert_eq!(e["args"]["kind"], "review");
    assert_eq!(e["args"]["step_run_id"], approve.id.as_str());
    assert!(audited_clean(&e["args"]["taint"]), "{e}");
    assert!(!e.to_string().contains(&item.body), "never the body: {e}");
    assert_eq!(
        p.store.reader().unwrap().verify_audit_chain().unwrap(),
        None
    );
}

/// Row 9 through `agentic run` on the fixture's `gated` workflow: `run
/// <id> waiting_human` and `Review item <id> waits for you in the Inbox` on
/// stdout, exit 0 (waiting is not failing; Decision 4). The owner's seat
/// has no model and none is asked for it.
#[tokio::test]
async fn the_cli_reports_the_waiting_item_and_exits_zero() {
    let cli = cli();
    let root = cli.copy_of("hand-offs");
    let llm = texts(&["the version to approve", "never"]);

    let ran = cli.run(&root, "gated", local(llm.clone())).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let (id, status) = ran.run_line();
    assert_eq!(status, "waiting_human", "{}", ran.all());
    assert_eq!(llm.calls(), 1, "only the first step calls a model");
    let j = cli.journal(&root);
    assert_eq!(j.items.len(), 1, "{:#?}", j.items);
    let item = &j.items[0];
    let line = ran
        .out
        .lines()
        .find(|l| l.starts_with("Review item "))
        .unwrap_or_else(|| panic!("no `Review item <id> …` line: {}", ran.all()));
    assert_eq!(
        line,
        format!("Review item {} waits for you in the Inbox", item.id)
    );
    assert_eq!(j.runs[0].id, id);
    assert_eq!(j.runs[0].status, "running");
    assert_eq!(j.attempt("approve", 1).status, "waiting_human");
}

/// Row 10: a gate whose payload's producer is tainted makes a tainted
/// Review item, and the gate's own attempt is tainted (AD-10, AD-15: from
/// the first build that serves the Inbox UI, resolving it needs the UI
/// class).
#[tokio::test]
async fn a_tainted_payload_makes_a_tainted_review_item() {
    let p = project();
    let plan = plan(vec![
        poisoned(step(
            "draft",
            texts(&["a version that read a poisoned page"]),
        )),
        gate("approve", never()),
        step("publish", never()),
    ]);

    let outcome = run_on(&p.store, &plan).await;

    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    let j = journal(&p.db);
    assert_eq!(j.attempt("draft", 1).taint, Taint::Tainted);
    assert_eq!(j.attempt("approve", 1).taint, Taint::Tainted);
    assert_eq!(j.items.len(), 1);
    assert_eq!(j.items[0].taint, Taint::Tainted);
    let created = j.entries("inbox.created");
    assert!(
        audited_tainted(&created[0]["args"]["taint"]),
        "{created:#?}"
    );
}

/// Row 11: no model output decides a gate. Step 1 says "approved", or
/// writes `{"approve": true}`: the gate still waits, its item stays open
/// and no later step runs.
#[tokio::test]
async fn no_model_output_resolves_a_gate() {
    for said in [
        "approved",
        r#"{"approve":true}"#,
        r#"{"verdict":"approve"}"#,
    ] {
        let p = project();
        let after = never();
        let plan = gated(texts(&[said]), never(), after.clone());

        let outcome = run_on(&p.store, &plan).await;

        assert_eq!(outcome.status, "waiting_human", "{said}: {outcome:?}");
        let j = journal(&p.db);
        assert_eq!(j.attempt("approve", 1).status, "waiting_human", "{said}");
        assert_eq!(j.items.len(), 1, "{said}");
        assert_eq!(j.items[0].status, "open", "{said}");
        assert_eq!(j.attempt("publish", 1).status, "queued", "{said}");
        assert_eq!(after.calls(), 0, "{said}");
    }
}

/// Rows 12 and 27: the run stops at the gate and the process ends. The
/// store is reopened: the step is still `waiting_human`, the item still
/// open and unchanged. The pause command sent again for the same gate
/// attempt (as a resumed engine would, with a new item id or the same one)
/// changes nothing: one item, one `inbox.created` entry.
#[tokio::test]
async fn a_waiting_gate_survives_a_restart_without_a_duplicate() {
    let p = project();
    let plan = gated(texts(&["v"]), never(), never());
    let outcome = run_on(&p.store, &plan).await;
    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    let before = journal(&p.db);
    let Project { dir, db, store } = p;
    drop(store);

    let store = Store::open(&db).unwrap();
    let after_open = journal(&db);
    assert_eq!(after_open.steps, before.steps, "reopening changes nothing");
    assert_eq!(after_open.items, before.items);

    let (draft, approve) = (before.attempt("draft", 1), before.attempt("approve", 1));
    let original = before.items[0].clone();
    for id in [new_id(), original.id.clone()] {
        let again = WriteCommand::PauseStep {
            step_run_id: approve.id.clone(),
            inputs: vec![StepInput {
                kind: "step_output".into(),
                from_step_run: Some(draft.id.clone()),
                from_item: None,
                from_row: None,
                label: "step:draft@1".into(),
                taint: Taint::Clean,
            }],
            taint: Taint::Clean,
            item: NewInboxItem {
                id,
                kind: InboxKind::Review,
                payload: Some(draft.id.clone()),
                taint: Taint::Clean,
                title: "Review the version".into(),
                body: "A second pause for the same gate attempt.".into(),
                max_returns: Some(2),
            },
        };
        // Refused or a no-change: either way nothing changes.
        let _ = store.writer().exec(engine(), again);
        let now = journal(&db);
        assert_eq!(now.items, vec![original.clone()], "one item, unchanged");
        assert_eq!(now.steps, before.steps);
        assert_eq!(now.entries("inbox.created").len(), 1);
    }
    drop(dir);
}

/// Row 27 with the 3.2 server rig: a run waits at a gate before the
/// server's watcher starts; its first look records what is there and
/// publishes nothing, so the waiting step is not announced again, and the
/// watcher changes nothing.
#[tokio::test]
async fn the_server_publishes_nothing_old_on_its_first_look() {
    let rig = server::Rig::with_project().await;
    let store = rig.store.as_ref().expect("a project");
    let outcome = run_on(store, &gated(texts(&["v"]), never(), never())).await;
    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    let db = rig.runtime.as_ref().unwrap().state_db();
    let before = journal(&db);

    let _watch = am_server::watch_runs(&rig.state, Duration::from_millis(50));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let s = rig.sign_in().await;
    let mut ws = rig.events_with_session(&s, Some(0)).await;
    let got = ws.take(10, Duration::from_millis(500)).await;

    let published: Vec<_> = got
        .iter()
        .filter(|e| e.kind == "run.updated" || e.kind == "step.updated")
        .collect();
    assert!(
        published.is_empty(),
        "the first look publishes nothing: {published:?}"
    );
    ws.close().await;
    let after = journal(&db);
    assert_eq!(after.steps, before.steps);
    assert_eq!(after.items, before.items);
}

/// Row 32 (owner condition 4): a run waits at the gate; the config is then
/// edited by hand to remove `gate:` from that step, and then to move the
/// step to a seat that has a model, `agentic run` being invoked after each
/// edit. The waiting run is never touched: its gate step stays
/// `waiting_human`, its Review item stays open and unchanged, no later step
/// of it starts and no model call is made for it. Only a Human resolve
/// (story 3.4) moves it on. (What the edited config does to a new run is
/// the interim risk the owner accepted until epic 4 story 17; it is not
/// pinned here.) It must stay green once the gate exists.
#[tokio::test]
async fn editing_config_never_releases_a_waiting_gate() {
    let cli = cli();
    let root = cli.copy_of("hand-offs");
    let ran = cli
        .run(&root, "gated", local(texts(&["the version to approve"])))
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let (waiting, status) = ran.run_line();
    assert_eq!(status, "waiting_human");
    // The waiting run's rows, through a fresh read-only connection.
    let reader = |root: &std::path::Path| am_store::Reader::open(&cli.db(root)).unwrap();
    let steps_of = |root: &std::path::Path| reader(root).steps(&waiting).unwrap();
    let calls_of = |root: &std::path::Path| reader(root).calls_for_run(&waiting).unwrap();
    let items_of = |root: &std::path::Path| reader(root).inbox_items(&waiting).unwrap();
    let (steps0, items0, calls0) = (steps_of(&root), items_of(&root), calls_of(&root));
    assert_eq!(items0.len(), 1);
    assert_eq!(calls0.len(), 1);

    let wf = root.join("workflows/gated.yaml");
    let text = std::fs::read_to_string(&wf).unwrap();
    // Edit 1: the gate removed from the step.
    let ungated = text.replace("    gate: review\n", "");
    assert_ne!(ungated, text);
    std::fs::write(&wf, &ungated).unwrap();
    let _ = cli
        .run(&root, "gated", local(texts(&["another version"])))
        .await;
    assert_eq!(steps_of(&root), steps0, "after removing the gate");
    assert_eq!(items_of(&root), items0);
    assert_eq!(calls_of(&root), calls0);

    // Edit 2: the step moved to a seat that has a model.
    let moved = ungated.replace("    seat: owner\n", "    seat: second\n");
    assert_ne!(moved, ungated);
    std::fs::write(&wf, &moved).unwrap();
    let _ = cli
        .run(
            &root,
            "gated",
            local(texts(&[
                "a third version",
                "approved by a model",
                "prepared",
            ])),
        )
        .await;
    assert_eq!(steps_of(&root), steps0, "after moving the gate step");
    assert_eq!(items_of(&root), items0);
    assert_eq!(calls_of(&root), calls0);
    let j = cli.journal(&root);
    let run = j.runs.iter().find(|r| r.id == waiting).unwrap();
    assert_eq!(run.status, "running");
    assert_eq!(run.finished_at, None);
    let approve = steps0
        .iter()
        .find(|s| s.step_key == "approve")
        .unwrap()
        .clone();
    assert_eq!(approve.status, "waiting_human");
    assert_eq!(items0[0].status, "open");
}

// ------------------------------------------------------- review round 1
//
// Story 3.3 review patch round 1 (coordinator's triage, 2026-11-23): P3
// (a gate's attempt passes through `running`, AD-8) and P11 (row 27:
// `agentic status` shows the open Review item). Written before the
// patches.

/// P3: the gate's attempt starts before it waits (AD-8 has no edge from
/// `queued` to `waiting_human`): while it waits, its `started_at` is set
/// and its finish is not; its start is audited before its wait.
#[tokio::test]
async fn a_gate_attempt_starts_before_it_waits() {
    let p = project();
    let outcome = run_on(&p.store, &gated(texts(&["v"]), never(), never())).await;
    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");

    let j = journal(&p.db);
    let approve = j.attempt("approve", 1);
    assert_eq!(approve.status, "waiting_human");
    assert!(approve.started_at.is_some(), "{approve:?}");
    assert_eq!(approve.finished_at, None);
    let on_gate: Vec<String> = j
        .audit
        .iter()
        .filter(|e| e["target"] == approve.id.as_str())
        .map(|e| e["action"].as_str().unwrap_or_default().to_string())
        .collect();
    let started = on_gate.iter().position(|a| a == "step.started");
    let waiting = on_gate.iter().position(|a| a == "step.waiting");
    assert!(
        matches!((started, waiting), (Some(s), Some(w)) if s < w),
        "{on_gate:?}"
    );
}

/// P11, row 27: after `agentic run gated` stops at the gate, `agentic
/// status` shows the waiting step and its open Review item by id.
#[tokio::test]
async fn the_status_shows_the_open_review_item() {
    let cli = cli();
    let root = cli.copy_of("hand-offs");
    let ran = cli
        .run(&root, "gated", local(texts(&["the version to approve"])))
        .await;
    assert_eq!(ran.code, 0, "{}", ran.all());
    let item = cli.journal(&root).items[0].clone();

    let status = cli.status(&root);

    assert_eq!(status.code, 0, "{}", status.all());
    let line = status
        .out
        .lines()
        .find(|l| l.contains(&item.id))
        .unwrap_or_else(|| panic!("status names the open item {}: {}", item.id, status.out));
    assert!(line.contains("Review"), "{line}");
    assert!(
        status
            .out
            .lines()
            .any(|l| l.contains("approve") && l.contains("waiting_human")),
        "{}",
        status.out
    );
}
