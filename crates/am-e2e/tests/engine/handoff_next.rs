//! Epic 3 story 3, the `next` hand-off (AD-8, AD-36, AD-27; matrix rows 1
//! and 2): every step attempt is a step-run row with an `attempt` number;
//! steps run in order by `next`, the default; the following step receives
//! the previous step's committed output as one input, journaled as a
//! `step_inputs` row before its call, and, the producer being untainted, as
//! a `TrustedInput` part labelled `step:<key>@<attempt>` after its
//! instruction (AD-10, amended 2026-11-23). `next` carries only the
//! immediately previous step's output (Question 4 (a)). Written before the
//! hand-offs exist; the test API is in the story plan's Implementation Notes.
#![forbid(unsafe_code)]

#[path = "support/mod.rs"]
mod support;

use std::sync::Arc;

use am_domain::{Part, Taint};
use am_store::StepInput;
use am_testkit::{FakeLlm, ScriptedTurn};
use support::*;
use tokio_util::sync::CancellationToken;

/// Row 1, Next: three steps by `next`; each runs once, in order, as
/// attempt 1, and succeeds; the run succeeds; no Inbox item.
#[tokio::test]
async fn steps_run_in_order_each_as_attempt_one() {
    let p = project();
    let (a, b, c) = (texts(&["a out"]), texts(&["b out"]), texts(&["c out"]));
    let plan = plan(vec![
        step("draft", a.clone()),
        step("edit", b.clone()),
        step("check", c.clone()),
    ]);

    let outcome =
        am_engine::run::run_plan(&p.store, owner(), "manual", &plan, CancellationToken::new())
            .await
            .unwrap();

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert_eq!(outcome.reason, None);
    assert_eq!((a.calls(), b.calls(), c.calls()), (1, 1, 1));
    let j = journal(&p.db);
    assert_eq!(
        j.shape(),
        vec![
            ("draft".to_string(), 1, "succeeded".to_string()),
            ("edit".to_string(), 1, "succeeded".to_string()),
            ("check".to_string(), 1, "succeeded".to_string()),
        ]
    );
    assert_eq!(
        j.steps
            .iter()
            .map(|s| (s.seq, s.output.as_deref()))
            .collect::<Vec<_>>(),
        vec![(0, Some("a out")), (1, Some("b out")), (2, Some("c out"))]
    );
    // In order: each step started after the one before it finished.
    for pair in j.steps.windows(2) {
        assert!(
            pair[0].finished_at.as_deref().unwrap() <= pair[1].started_at.as_deref().unwrap(),
            "{pair:#?}"
        );
    }
    assert!(j.steps.iter().all(|s| s.taint == Taint::Clean), "{j:#?}");
    assert_eq!(j.runs[0].status, "succeeded");
    assert!(j.items.is_empty(), "no Inbox item: {:#?}", j.items);
    assert_eq!(
        p.store.reader().unwrap().verify_audit_chain().unwrap(),
        None
    );
}

/// Row 2, Next carries the output: step 2's request holds step 1's output
/// as one trusted input labelled `step:draft@1`, after its instruction;
/// the input is a `step_inputs` row (kind `step_output`, from step 1's
/// attempt, taint 0) committed before step 2's call. Step 3 receives step
/// 2's output only. Step 1 has no input.
#[tokio::test]
async fn the_previous_output_arrives_as_one_trusted_input() {
    let p = project();
    let first = texts(&["the first version"]);
    let second = Spy::new(
        &p.db,
        Arc::new(FakeLlm::new(vec![ScriptedTurn::text("the edited version")])),
    );
    let third = texts(&["looks done"]);
    let plan = plan(vec![
        step("draft", first.clone()),
        step("edit", second.clone()),
        step("check", third.clone()),
    ]);

    let outcome =
        am_engine::run::run_plan(&p.store, owner(), "manual", &plan, CancellationToken::new())
            .await
            .unwrap();
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");

    // Step 1: its instruction only.
    let parts = user_parts(&first.requests()[0]);
    assert_eq!(parts.len(), 1, "{parts:#?}");
    assert!(matches!(&parts[0], Part::Instruction(i) if i.text() == instruction_of("draft")));

    // Step 2: its instruction, then step 1's output as one trusted part.
    let parts = user_parts(&second.requests()[0]);
    assert_eq!(parts.len(), 2, "{parts:#?}");
    assert!(matches!(&parts[0], Part::Instruction(i) if i.text() == instruction_of("edit")));
    let t = trusted(&parts, "step:draft@1");
    assert_eq!(t.text(), "the first version");
    assert!(
        matches!(&parts[1], Part::Trusted(_)),
        "after the instruction"
    );
    // Never in the system position (AD-10).
    assert!(
        carriers(&second.requests()[0], "the first version")
            .iter()
            .all(|p| matches!(p, Part::Trusted(_))),
        "{:#?}",
        second.requests()[0]
    );

    // Step 3: step 2's output only (Question 4 (a)).
    let request = &third.requests()[0];
    let parts = user_parts(request);
    assert_eq!(trusted(&parts, "step:edit@1").text(), "the edited version");
    assert!(
        carriers(request, "the first version").is_empty(),
        "next carries only the previous step's output: {request:#?}"
    );

    // The journal: one input row per step after the first, committed
    // before that step's call.
    let j = journal(&p.db);
    let (draft, edit, check) = (
        j.attempt("draft", 1),
        j.attempt("edit", 1),
        j.attempt("check", 1),
    );
    assert!(j.inputs_of(&draft.id).is_empty(), "{:#?}", j.inputs);
    assert_eq!(
        j.inputs_of(&edit.id),
        vec![StepInput {
            kind: "step_output".into(),
            from_step_run: Some(draft.id.clone()),
            label: "step:draft@1".into(),
            taint: Taint::Clean,
        }]
    );
    assert_eq!(
        j.inputs_of(&check.id),
        vec![StepInput {
            kind: "step_output".into(),
            from_step_run: Some(edit.id.clone()),
            label: "step:edit@1".into(),
            taint: Taint::Clean,
        }]
    );
    let at_call = &second.seen()[0];
    let edit_then = at_call.attempt("edit", 1);
    assert_eq!(edit_then.status, "running");
    assert_eq!(at_call.inputs_of(&edit_then.id), j.inputs_of(&edit.id));
    // The output is stored once, in the producer's row; the input refers
    // to it (AD-43: never stored twice).
    assert_eq!(draft.output.as_deref(), Some("the first version"));
}

/// Rows 1 and 2 through `agentic run`, in process, on the fixture's
/// three-step workflow `relay` (all three seats on the fixture's
/// `local-lmstudio` Account, so one scripted model plays the three turns
/// in order). The third step declares a loop-back and outputs an empty
/// reasons list, which means `next`: the run succeeds with attempt 1 of
/// each step, `run <id> succeeded`, exit 0; the second request holds the
/// first step's output as a trusted part labelled `step:draft@1`.
#[tokio::test]
async fn the_cli_runs_the_three_step_fixture() {
    let cli = cli();
    let root = cli.copy_of("hand-offs");
    let llm = texts(&[
        "the first version",
        "the edited version",
        r#"{"reasons":[]}"#,
    ]);

    let ran = cli.run(&root, "relay", local(llm.clone())).await;

    assert_eq!(ran.code, 0, "{}", ran.all());
    let (id, status) = ran.run_line();
    assert_eq!(status, "succeeded", "{}", ran.all());
    assert_eq!(llm.calls(), 3);
    let parts = user_parts(&llm.requests()[1]);
    assert_eq!(trusted(&parts, "step:draft@1").text(), "the first version");
    let j = cli.journal(&root);
    assert_eq!(j.runs[0].id, id);
    assert_eq!(
        j.shape(),
        vec![
            ("draft".to_string(), 1, "succeeded".to_string()),
            ("edit".to_string(), 1, "succeeded".to_string()),
            ("check".to_string(), 1, "succeeded".to_string()),
        ]
    );
    assert!(j.items.is_empty());
}

// ------------------------------------------------------- review round 1
//
// Story 3.3 review patch round 1 (coordinator's triage, 2026-11-23): P11,
// row 26 (and row 30) through `agentic run`: a hand-off config error, or a
// run-graph key, starts no run at all.

/// P11, rows 26 and 30: `agentic run` on the fixture with a hand-off error
/// (`loop_back` to a step that is not earlier, a maximum out of range, a
/// gate first, a cap out of range) or a run-graph key (`handoff`) exits 1,
/// points at `agentic validate` (or names the key), asks for no model and
/// writes no run row.
#[tokio::test]
async fn a_hand_off_config_error_or_a_run_graph_key_starts_no_run() {
    let cases: [(&str, &str, &str, &str); 5] = [
        (
            "relay",
            "workflows/relay.yaml",
            "loop_back: { to: draft, reasons: reasons, max: 2 }",
            "loop_back: { to: check, reasons: reasons, max: 2 }",
        ),
        (
            "relay",
            "workflows/relay.yaml",
            "loop_back: { to: draft, reasons: reasons, max: 2 }",
            "loop_back: { to: draft, reasons: reasons, max: 9 }",
        ),
        (
            "capped",
            "workflows/capped.yaml",
            "iterations: 3",
            "iterations: 500",
        ),
        (
            "gated",
            "workflows/gated.yaml",
            "  - id: draft\n    seat: first\n    instruction: Write the first version.\n  - id: approve\n    seat: owner\n    instruction: Approve or return the version.\n    gate: review\n",
            "  - id: approve\n    seat: owner\n    instruction: Approve or return the version.\n    gate: review\n  - id: draft\n    seat: first\n    instruction: Write the first version.\n",
        ),
        (
            "gated",
            "workflows/gated.yaml",
            "    seat: first\n",
            "    seat: first\n    handoff:\n      to: core/gated/publish\n      payload: document:note\n",
        ),
    ];
    for (workflow, file, from, to) in cases {
        let cli = cli();
        let root = cli.copy_of("hand-offs");
        let path = root.join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(from), "{file}: {from}");
        std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
        let llm = texts(&["never"]);

        let ran = cli.run(&root, workflow, local(llm.clone())).await;

        assert_eq!(ran.code, 1, "{to}: {}", ran.all());
        assert!(
            ran.err.contains("agentic validate") || ran.err.contains("cannot honour"),
            "{to}: {}",
            ran.all()
        );
        assert!(
            !ran.out.lines().any(|l| l.starts_with("run ")),
            "{to}: {}",
            ran.out
        );
        assert_eq!(llm.calls(), 0, "{to}");
        let runs = cli
            .layout(&root)
            .map(|l| l.state_db())
            .filter(|db| db.is_file())
            .map(|db| am_store::Reader::open(&db).unwrap().runs().unwrap().len())
            .unwrap_or(0);
        assert_eq!(runs, 0, "{to}: no run row");
    }
}
