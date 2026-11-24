//! Epic 3 story 3, the `loop_back` hand-off (AD-8, AD-36; matrix rows 3 to
//! 8 and 18): a step that declares `loop_back` reads its final message as a
//! JSON object whose named field (default `reasons`) is a list of strings.
//! A non-empty list closes the producing attempt `looped_back` (its output
//! stored) and queues new attempts from the target step through the
//! producer, in one transaction; the reasons reach the target's next
//! attempt as a trusted input labelled `loop-back:<key>@<attempt>` (the
//! producer is untainted here; tainted producers are in
//! `tests/trust/taint_handoff_chain.rs`). An empty list means `next`. The
//! count is the producer's `looped_back` attempts in the journal; past
//! `max` (default 2) the step fails with one Failure Inbox item in engine
//! words. Old attempts stay in the journal unchanged. Written before the
//! hand-offs exist.
#![forbid(unsafe_code)]

#[path = "support/mod.rs"]
mod support;

#[path = "../trust/support/server.rs"]
mod server;

use std::sync::Arc;

use am_domain::{InboxKind, Part, Taint};
use am_store::StepInput;
use am_testkit::{FakeLlm, ScriptedTurn};
use support::*;
use tokio_util::sync::CancellationToken;

const R1_R2: &str = r#"{"reasons":["r1 tighten the opening","r2 cite the source"]}"#;
const DONE: &str = r#"{"reasons":[]}"#;

async fn run(p: &Project, plan: &am_engine::run::RunPlan) -> am_engine::run::RunOutcome {
    am_engine::run::run_plan(&p.store, owner(), "manual", plan, CancellationToken::new())
        .await
        .unwrap()
}

/// Row 3, the ticket's verify: step 3 outputs reasons once, then an empty
/// list. Attempt 1 of step 3 ends `looped_back` with its output stored;
/// attempts 2 of steps 1, 2 and 3 run; step 1's second request holds the
/// reasons as one trusted part labelled `loop-back:check@1`, journaled as a
/// `step_inputs` row (kind `loop_back`, from step 3's attempt 1, taint 0);
/// the run succeeds.
#[tokio::test]
async fn step_three_loops_back_to_step_one_with_its_reasons_as_a_trusted_input() {
    let p = project();
    let draft = texts(&["version one", "version two"]);
    let edit = texts(&["edited one", "edited two"]);
    let check = texts(&[R1_R2, DONE]);
    let plan = plan(vec![
        step("draft", draft.clone()),
        step("edit", edit.clone()),
        looping("check", check.clone(), "draft", 2),
    ]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert_eq!((draft.calls(), edit.calls(), check.calls()), (2, 2, 2));
    let j = journal(&p.db);
    assert_eq!(
        j.shape(),
        vec![
            ("draft".to_string(), 1, "succeeded".to_string()),
            ("draft".to_string(), 2, "succeeded".to_string()),
            ("edit".to_string(), 1, "succeeded".to_string()),
            ("edit".to_string(), 2, "succeeded".to_string()),
            ("check".to_string(), 1, "looped_back".to_string()),
            ("check".to_string(), 2, "succeeded".to_string()),
        ]
    );
    let check1 = j.attempt("check", 1);
    assert_eq!(
        check1.output.as_deref(),
        Some(R1_R2),
        "the reasons are kept"
    );
    assert!(check1.finished_at.is_some());

    // The reasons reach step 1's second attempt as one trusted part, after
    // its instruction; its first attempt had no input.
    assert_eq!(user_parts(&draft.requests()[0]).len(), 1);
    let parts = user_parts(&draft.requests()[1]);
    assert!(matches!(&parts[0], Part::Instruction(i) if i.text() == instruction_of("draft")));
    let reasons = trusted(&parts, "loop-back:check@1");
    for r in ["r1 tighten the opening", "r2 cite the source"] {
        assert!(reasons.text().contains(r), "{}", reasons.text());
    }
    // The journal says so: a trusted `loop_back` input (the verify).
    let draft2 = j.attempt("draft", 2);
    assert_eq!(
        j.inputs_of(&draft2.id),
        vec![StepInput {
            kind: "loop_back".into(),
            from_step_run: Some(check1.id.clone()),
            from_item: None,
            label: "loop-back:check@1".into(),
            taint: Taint::Clean,
        }]
    );
    // The next attempts are fed by `next` from the new attempts.
    let edit2 = j.attempt("edit", 2);
    assert_eq!(
        j.inputs_of(&edit2.id),
        vec![StepInput {
            kind: "step_output".into(),
            from_step_run: Some(draft2.id.clone()),
            from_item: None,
            label: "step:draft@2".into(),
            taint: Taint::Clean,
        }]
    );
    assert_eq!(
        trusted(&user_parts(&edit.requests()[1]), "step:draft@2").text(),
        "version two"
    );
    assert!(j.steps.iter().all(|s| s.taint == Taint::Clean));
    assert!(j.items.is_empty(), "{:#?}", j.items);
    assert_eq!(j.runs[0].status, "succeeded");
    // The loop-back is audited, without the reasons' text.
    let looped: Vec<_> = j
        .audit
        .iter()
        .filter(|e| {
            e["action"]
                .as_str()
                .is_some_and(|a| a.contains("step.looped_back"))
        })
        .collect();
    assert_eq!(looped.len(), 1, "{:#?}", j.audit);
    assert!(
        !looped[0].to_string().contains("tighten the opening"),
        "{}",
        looped[0]
    );
    assert_eq!(
        p.store.reader().unwrap().verify_audit_chain().unwrap(),
        None
    );
}

/// Row 4: an empty reasons list is `next`: no new attempt.
#[tokio::test]
async fn an_empty_reasons_list_moves_on() {
    let p = project();
    let draft = texts(&["version one"]);
    let check = texts(&[DONE]);
    let after = texts(&["published"]);
    let plan = plan(vec![
        step("draft", draft.clone()),
        looping("check", check.clone(), "draft", 2),
        step("publish", after.clone()),
    ]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert_eq!((draft.calls(), check.calls(), after.calls()), (1, 1, 1));
    let j = journal(&p.db);
    assert_eq!(
        j.shape(),
        vec![
            ("draft".to_string(), 1, "succeeded".to_string()),
            ("check".to_string(), 1, "succeeded".to_string()),
            ("publish".to_string(), 1, "succeeded".to_string()),
        ]
    );
    // The step after it receives its output by `next`, as any step's.
    assert_eq!(
        trusted(&user_parts(&after.requests()[0]), "step:check@1").text(),
        DONE
    );
}

/// Row 5: with `max: 2`, step 3 asks three times. The first two asks loop
/// back; the third ends step 3 `failed`, fails the run and raises one
/// Failure item in engine words: it names the step and the maximum 2 and
/// points at step 3's last attempt by id; the reasons stay in that
/// attempt's stored output, never in the item.
#[tokio::test]
async fn looping_past_the_maximum_fails_with_one_failure_item() {
    let p = project();
    let draft = texts(&["v"]);
    let edit = texts(&["e"]);
    let ask = r#"{"reasons":["still not right, says the model"]}"#;
    let check = texts(&[ask]);
    let plan = plan(vec![
        step("draft", draft.clone()),
        step("edit", edit.clone()),
        looping("check", check.clone(), "draft", 2),
    ]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let reason = outcome.reason.clone().expect("the reason, in words");
    assert!(
        reason.contains("check") && names_number(&reason, 2),
        "{reason}"
    );
    assert_eq!((draft.calls(), edit.calls(), check.calls()), (3, 3, 3));
    let j = journal(&p.db);
    let statuses: Vec<String> = j
        .attempts("check")
        .iter()
        .map(|s| s.status.clone())
        .collect();
    assert_eq!(statuses, vec!["looped_back", "looped_back", "failed"]);
    let last = j.attempt("check", 3);
    assert_eq!(last.output.as_deref(), Some(ask), "the reasons stay stored");
    assert_eq!(j.runs[0].status, "failed");
    assert!(j.runs[0].finished_at.is_some());

    assert_eq!(j.items.len(), 1, "{:#?}", j.items);
    let item = &j.items[0];
    assert_eq!(item.kind, InboxKind::Failure);
    assert_eq!(item.status, "open");
    assert_eq!(item.run_id, j.runs[0].id);
    assert_eq!(item.step_run_id, last.id);
    assert!(item.body.contains("check"), "names the step: {}", item.body);
    assert!(
        names_number(&item.body, 2),
        "names the maximum: {}",
        item.body
    );
    assert!(
        item.body.contains(&last.id),
        "points at the attempt: {}",
        item.body
    );
    for text in [&item.title, &item.body] {
        assert!(!text.contains("says the model"), "no model text: {text}");
    }
    assert_eq!(
        (item.resolved_at.clone(), item.resolved_by.clone()),
        (None, None)
    );
    // Audited without the body.
    let created = j.entries("inbox.created");
    assert_eq!(created.len(), 1, "{:#?}", j.audit);
    assert_eq!(created[0]["target"], item.id.as_str());
    assert_eq!(created[0]["args"]["kind"], "failure");
    assert!(!created[0].to_string().contains(&item.body));
}

/// Row 6: `max: 0` fails on the first ask, with one Failure item, and
/// nothing is re-run.
#[tokio::test]
async fn a_maximum_of_zero_fails_on_the_first_ask() {
    let p = project();
    let draft = texts(&["v"]);
    let check = texts(&[r#"{"reasons":["one more pass"]}"#]);
    let plan = plan(vec![
        step("draft", draft.clone()),
        looping("check", check.clone(), "draft", 0),
    ]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    assert_eq!((draft.calls(), check.calls()), (1, 1));
    let j = journal(&p.db);
    assert_eq!(
        j.shape(),
        vec![
            ("draft".to_string(), 1, "succeeded".to_string()),
            ("check".to_string(), 1, "failed".to_string()),
        ]
    );
    assert_eq!(j.items.len(), 1, "{:#?}", j.items);
    assert_eq!(j.items[0].kind, InboxKind::Failure);
    assert_eq!(j.items[0].step_run_id, j.attempt("check", 1).id);
    assert!(names_number(&j.items[0].body, 0), "{}", j.items[0].body);
}

/// Row 7: a step that declares `loop_back` whose final message is not a
/// JSON object with the named field as a list of strings fails the step in
/// words naming the field and the shape expected (fail closed until story
/// 3.5's schemas), with no Failure item (Question 6) and no re-run. A step
/// that names its own field (`issues`) is read by that field.
#[tokio::test]
async fn a_malformed_loop_back_output_fails_the_step_in_words() {
    let cases: [(&str, &str); 5] = [
        ("reasons", "Looks fine to me."),
        ("reasons", r#"{"verdict":"ok"}"#),
        ("reasons", r#"{"reasons":[1,2]}"#),
        ("reasons", r#"{"reasons":"tighten it"}"#),
        ("issues", r#"{"reasons":["tighten it"]}"#),
    ];
    for (field, output) in cases {
        let p = project();
        let draft = texts(&["v"]);
        let check = texts(&[output]);
        let mut checking = looping("check", check.clone(), "draft", 2);
        checking.hand_off = am_engine::run::HandOff::LoopBack {
            to: "draft".into(),
            field: field.into(),
            max: 2,
        };
        let plan = plan(vec![step("draft", draft.clone()), checking]);

        let outcome = run(&p, &plan).await;

        assert_eq!(outcome.status, "failed", "{output}: {outcome:?}");
        let reason = outcome.reason.clone().expect("in words");
        assert!(
            reason.contains(&format!("`{field}`")) || reason.contains(&format!("'{field}'")),
            "{output}: names the field: {reason}"
        );
        assert!(
            reason.contains("list of strings"),
            "{output}: names the shape: {reason}"
        );
        assert_eq!((draft.calls(), check.calls()), (1, 1), "{output}");
        let j = journal(&p.db);
        assert_eq!(j.attempt("check", 1).status, "failed", "{output}");
        assert_eq!(j.attempts("draft").len(), 1, "{output}: no re-run");
        assert!(j.items.is_empty(), "{output}: no Failure item");
        assert_eq!(j.runs[0].status, "failed", "{output}");
    }
    // The control: the named field is read.
    let p = project();
    let draft = texts(&["v", "v2"]);
    let mut checking = looping(
        "check",
        texts(&[r#"{"issues":["tighten it"]}"#, r#"{"issues":[]}"#]),
        "draft",
        2,
    );
    checking.hand_off = am_engine::run::HandOff::LoopBack {
        to: "draft".into(),
        field: "issues".into(),
        max: 2,
    };
    let outcome = run(&p, &plan(vec![step("draft", draft.clone()), checking])).await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert_eq!(draft.calls(), 2);
}

/// Row 8: after a loop-back, the attempt-1 rows of steps 1 to 3 are as they
/// were when the loop-back committed (output, status, times, taint); the
/// new attempts are new rows with their own ids.
#[tokio::test]
async fn earlier_attempts_stay_in_the_journal() {
    let p = project();
    let draft = Spy::new(
        &p.db,
        Arc::new(FakeLlm::new(vec![
            ScriptedTurn::text("version one"),
            ScriptedTurn::text("version two"),
        ])),
    );
    let edit = texts(&["edited one", "edited two"]);
    let check = texts(&[R1_R2, DONE]);
    let plan = plan(vec![
        step("draft", draft.clone()),
        step("edit", edit.clone()),
        looping("check", check.clone(), "draft", 2),
    ]);

    let outcome = run(&p, &plan).await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");

    // When step 1's second attempt was called, the loop-back had committed.
    let at_second = &draft.seen()[1];
    let then: Vec<_> = at_second
        .steps
        .iter()
        .filter(|s| s.attempt == 1)
        .cloned()
        .collect();
    assert_eq!(then.len(), 3, "{:#?}", at_second.steps);
    assert_eq!(at_second.attempt("check", 1).status, "looped_back");
    assert_eq!(at_second.attempt("draft", 2).status, "running");
    let now = journal(&p.db);
    let kept: Vec<_> = now
        .steps
        .iter()
        .filter(|s| s.attempt == 1)
        .cloned()
        .collect();
    assert_eq!(kept, then, "attempt-1 rows are never rewritten");
    let ids: std::collections::BTreeSet<&str> = now.steps.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids.len(), 6, "every attempt is its own row");
    // The steps are listed in workflow order, each step's attempts in order.
    assert_eq!(
        now.steps
            .iter()
            .map(|s| (s.seq, s.attempt))
            .collect::<Vec<_>>(),
        vec![(0, 1), (0, 2), (1, 1), (1, 2), (2, 1), (2, 2)]
    );
}

/// Row 18 (loop-backs): the count the engine acts on is the producer's
/// `looped_back` attempts in the journal. At each of step 3's calls the
/// journal already holds one `looped_back` attempt fewer than that call's
/// attempt number; at the end a fresh reader counts exactly `max` of them,
/// and the Failure item names that maximum.
#[tokio::test]
async fn the_loop_back_count_comes_from_the_journal() {
    let p = project();
    let check = Spy::new(
        &p.db,
        Arc::new(FakeLlm::new(vec![ScriptedTurn::text(
            r#"{"reasons":["again"]}"#,
        )])),
    );
    let plan = plan(vec![
        step("draft", texts(&["v"])),
        looping("check", check.clone(), "draft", 2),
    ]);

    let outcome = run(&p, &plan).await;
    assert_eq!(outcome.status, "failed", "{outcome:?}");

    let seen = check.seen();
    assert_eq!(seen.len(), 3);
    for (i, at) in seen.iter().enumerate() {
        let attempt = i as i64 + 1;
        let looped = at
            .attempts("check")
            .iter()
            .filter(|s| s.status == "looped_back")
            .count();
        assert_eq!(looped as i64, attempt - 1, "at check's call {attempt}");
        assert_eq!(at.attempt("check", attempt).status, "running");
    }
    let fresh = journal(&p.db);
    let looped = fresh
        .attempts("check")
        .iter()
        .filter(|s| s.status == "looped_back")
        .count();
    assert_eq!(looped, 2);
    assert_eq!(fresh.items.len(), 1);
    assert!(
        names_number(&fresh.items[0].body, 2),
        "{}",
        fresh.items[0].body
    );
}

// ------------------------------------------------------- review round 1
//
// Story 3.3 review patch round 1 (coordinator's triage, 2026-11-23): P11,
// row 31: the 3.2 server's run watcher publishes each new attempt row of a
// loop-back as `step.updated`, once per state it saw, never twice.

/// P11, row 31: with `agentic serve`'s watcher looking at the project, a
/// run that loops back once makes six attempt rows; each is published as
/// `step.updated` (by its own id), its last event carries its final state,
/// and no state of a row is published twice.
#[tokio::test]
async fn each_new_attempt_row_is_published_once() {
    let rig = server::Rig::with_project().await;
    let _watch = am_server::watch_runs(&rig.state, std::time::Duration::from_millis(50));
    // Let the watcher take its first look (an empty project).
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let s = rig.sign_in().await;
    let mut ws = rig.events_with_session(&s, Some(0)).await;
    let store = rig.store.as_ref().expect("a project");
    let plan = plan(vec![
        step("draft", texts(&["version one", "version two"])),
        step("edit", texts(&["edited one", "edited two"])),
        looping("check", texts(&[R1_R2, DONE]), "draft", 2),
    ]);

    let outcome =
        am_engine::run::run_plan(store, owner(), "manual", &plan, CancellationToken::new())
            .await
            .unwrap();
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let events = ws.take(200, std::time::Duration::from_millis(600)).await;
    ws.close().await;

    let j = journal(&rig.runtime.as_ref().unwrap().state_db());
    assert_eq!(j.steps.len(), 6);
    let updates: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| e.kind == "step.updated")
        .map(|e| &e.data)
        .collect();
    for row in &j.steps {
        let mine: Vec<&&serde_json::Value> = updates
            .iter()
            .filter(|d| d["id"] == row.id.as_str())
            .collect();
        assert!(
            !mine.is_empty(),
            "{} attempt {} is published: {updates:#?}",
            row.step_key,
            row.attempt
        );
        assert_eq!(
            mine.last().unwrap()["status"],
            row.status.as_str(),
            "{} attempt {}: its last event is its final state",
            row.step_key,
            row.attempt
        );
        let mut seen = std::collections::BTreeSet::new();
        for d in &mine {
            assert!(
                seen.insert(d.to_string()),
                "{} attempt {} published twice with the same state: {d}",
                row.step_key,
                row.attempt
            );
        }
    }
}
