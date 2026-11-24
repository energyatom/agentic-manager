//! Trust test (AD-10, amended 2026-11-23; AD-27; epic 3 story 5): the type
//! wall between the three kinds of prompt content holds in what the engine
//! sends and in what the journal says it sent.
//!
//! - A seat's goal, backstory and attached skills (and the role text, CEO
//!   goals, guidance and memory notes, the two name lists and the step
//!   instruction) are `Instruction`s, config- or owner-authored text, in
//!   the system position, whatever the attempt read: a tainted attempt's
//!   goal row is still an `instruction` row, never tainted.
//! - No instruction or trusted row is ever tainted and no untrusted row is
//!   ever clean, in any run, and the writer refuses a prompt that says
//!   otherwise (a forged row cannot be journaled).
//! - A tainted producer's loop-back reasons or output reach the next
//!   attempt as fenced `UntrustedData` with the same label, never as an
//!   instruction or a trusted part; a Human's gate note stays trusted and
//!   neither adds nor clears taint.
//! - A plan input never becomes system text.
//!
//! The type wall itself (no `From`, `TryFrom` or `Into` between the three)
//! is am-domain's `compile_fail` doctests on `Instruction`; the source rule
//! is `cargo xtask check-deps`. FakeLlm only. Written before the journal
//! holds a prompt.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

use am_domain::{Actor, Decision, HumanActor, Part, REVIEWER_HAT, Taint, TrustedInput};
use am_engine::prompt::{StepPrompt, skill_label};
use am_engine::run::{HandOff, RunPlan, StepPlan, continue_run, run_plan};
use am_store::{NewPromptPart, PromptKind, PromptSlot, StepInput, WriteCommand};
use support::*;
use tokio_util::sync::CancellationToken;

const GOAL: &str = "goal-canary-77f1";
const BACKSTORY: &str = "backstory-canary-2c9e";
const SKILL: &str = "skill-canary-b04d";
const REASONS: &str = "reasons-canary-5a63";
const NOTE: &str = "tighten the summary";

fn seat_prompt() -> StepPrompt {
    StepPrompt::default()
        .with_goal(labelled("Goal", GOAL))
        .with_backstory(labelled("Backstory", BACKSTORY))
        .with_skill("brief", labelled(&skill_label("brief"), SKILL))
}

/// Every prompt row of every attempt of the run, with its attempt's taint.
fn all_rows(j: &Journal) -> Vec<(am_store::PromptPartRow, Taint)> {
    let mut out = Vec::new();
    for s in &j.steps {
        for row in j.parts_of(&s.id) {
            out.push((row, s.taint));
        }
    }
    out
}

/// No instruction row is tainted, no trusted row is tainted, no untrusted
/// row is clean; an assistant row carries its attempt's taint.
fn assert_the_wall(j: &Journal) {
    let rows = all_rows(j);
    assert!(!rows.is_empty(), "the journal holds prompts");
    for (row, attempt) in rows {
        match row.kind {
            PromptKind::Instruction | PromptKind::Trusted => {
                assert_eq!(row.taint, Taint::Clean, "{row:?}")
            }
            PromptKind::Untrusted => assert_eq!(row.taint, Taint::Tainted, "{row:?}"),
            PromptKind::Assistant => assert_eq!(row.taint, attempt, "{row:?}"),
        }
    }
}

fn plain_plan(steps: Vec<StepPlan>) -> RunPlan {
    plan(steps)
}

/// Rows 2 and 14: goal, backstory and a skill are `instruction` rows, in
/// the system position, with taint clear, on a clean attempt and on a
/// tainted one (the plan input tainted it): the attempt's taint is not the
/// part's.
#[tokio::test]
async fn the_goal_backstory_and_skill_parts_are_instructions_in_the_journal() {
    for tainted in [false, true] {
        let p = project();
        let llm = texts(&["done"]);
        let mut write = with_prompt(step("write", llm.clone()), seat_prompt());
        if tainted {
            write = poisoned(write);
        }

        let outcome = run_plan(
            &p.store,
            owner(),
            "manual",
            &plain_plan(vec![write]),
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(outcome.status, "succeeded", "{outcome:?}");
        let j = journal(&p.db);
        let row = j.attempt("write", 1);
        assert_eq!(row.taint == Taint::Tainted, tainted);
        let parts = j.parts_of(&row.id);
        for (slot, label) in [
            (PromptSlot::Goal, "Goal"),
            (PromptSlot::Backstory, "Backstory"),
            (PromptSlot::Skill, "skill:brief"),
        ] {
            let found: Vec<_> = parts.iter().filter(|p| p.slot == slot).collect();
            assert_eq!(found.len(), 1, "{slot:?}: {parts:#?}");
            assert_eq!(found[0].kind, PromptKind::Instruction, "tainted: {tainted}");
            assert_eq!(found[0].taint, Taint::Clean, "tainted: {tainted}");
            assert_eq!(found[0].label, label);
        }
        let request = llm.requests()[0].clone();
        for canary in [GOAL, BACKSTORY, SKILL] {
            assert_eq!(
                request
                    .system
                    .iter()
                    .filter(|i| i.text().contains(canary))
                    .count(),
                1,
                "{canary} is system text"
            );
            assert_eq!(
                carriers(&request, canary).len(),
                1,
                "{canary} is in no other part"
            );
        }
        assert_the_wall(&j);
    }
}

/// Row 14, the writer: a prompt that says an instruction was tainted, an
/// untrusted part was clean, a trusted part was tainted, or an untrusted
/// part sat in an instruction slot is refused, in words, with nothing
/// written; the engine's own runs, clean, tainted, looped back and gated,
/// journal rows that keep the wall. The journal cannot say a part was an
/// instruction when it was not.
#[tokio::test]
async fn no_instruction_row_is_ever_tainted_and_no_untrusted_row_is_clean() {
    // The writer refuses the forgeries.
    let p = project();
    p.store
        .writer()
        .exec(
            owner(),
            WriteCommand::CreateRun {
                run_id: "run-1".into(),
                workflow: "wf".into(),
                origin: "manual".into(),
                steps: vec!["a".into()],
                config_hash: Some(HASH.into()),
            },
        )
        .unwrap();
    let id = p.store.reader().unwrap().steps("run-1").unwrap()[0]
        .id
        .clone();
    let forged = |slot, kind, label: &str, taint, text: Option<&str>| NewPromptPart {
        slot,
        kind,
        label: label.into(),
        taint,
        text: text.map(str::to_string),
    };
    let page = StepInput {
        kind: "untrusted".into(),
        from_step_run: None,
        from_item: None,
        label: "test:page".into(),
        taint: Taint::Tainted,
    };
    let forgeries = [
        (
            "a tainted instruction",
            vec![],
            forged(
                PromptSlot::Goal,
                PromptKind::Instruction,
                "Goal",
                Taint::Tainted,
                Some("t"),
            ),
        ),
        (
            "a clean untrusted part",
            vec![page.clone()],
            forged(
                PromptSlot::Untrusted,
                PromptKind::Untrusted,
                "test:page",
                Taint::Clean,
                Some("t"),
            ),
        ),
        (
            "an untrusted part in an instruction slot",
            vec![page.clone()],
            forged(
                PromptSlot::Backstory,
                PromptKind::Untrusted,
                "Backstory",
                Taint::Tainted,
                Some("t"),
            ),
        ),
        (
            "an instruction in the untrusted slot",
            vec![page],
            forged(
                PromptSlot::Untrusted,
                PromptKind::Instruction,
                "test:page",
                Taint::Clean,
                Some("t"),
            ),
        ),
    ];
    for (what, inputs, part) in forgeries {
        let refused = p
            .store
            .writer()
            .exec(
                engine(),
                WriteCommand::StartAttempt {
                    step_run_id: id.clone(),
                    inputs,
                    taint: Taint::Clean,
                    prompt: vec![part],
                },
            )
            .expect_err(what);
        assert!(refused.to_string().contains("refused"), "{what}: {refused}");
    }
    let j = journal(&p.db);
    assert_eq!(j.steps[0].status, "queued", "nothing was written");
    assert!(j.parts_of(&id).is_empty());

    // The engine's own runs keep the wall.
    for tainted in [false, true] {
        let p = project();
        let mut a = with_prompt(
            step("a", texts(&["out of a", "out of a again"])),
            seat_prompt(),
        );
        if tainted {
            a = poisoned(a);
        }
        let check = texts(&[r#"{"reasons":["again"]}"#, r#"{"reasons":[]}"#]);
        let steps = vec![a, looping("b", check, "a", 1)];

        let outcome = run_plan(
            &p.store,
            owner(),
            "manual",
            &plain_plan(steps),
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(outcome.status, "succeeded", "{outcome:?}");
        assert_the_wall(&journal(&p.db));
    }
}

fn gate_max(key: &str, provider: std::sync::Arc<dyn am_domain::LlmProvider>, max: u32) -> StepPlan {
    StepPlan {
        hand_off: HandOff::Gate { max_returns: max },
        ..step(key, provider)
    }
}

/// Rows 9 and 10 with 3.3's and 3.4's constructors: a tainted producer's
/// loop-back reasons are fenced `UntrustedData` with their label (never an
/// instruction or a trusted part) and taint the target's new attempt; a
/// Human's gate note is trusted (`from_owner_note`, `Actor::Human` only)
/// and, on a tainted producer, neither clears the attempt's taint nor
/// becomes an instruction; no non-Human can make one.
#[tokio::test]
async fn a_tainted_producers_note_or_reason_is_never_an_instruction() {
    // The constructors, as the engine uses them.
    assert!(matches!(
        TrustedInput::from_step_output("loop-back:check@1", REASONS, Taint::Tainted),
        Part::Untrusted(_)
    ));
    assert!(matches!(
        TrustedInput::from_step_output("loop-back:check@1", REASONS, Taint::Clean),
        Part::Trusted(_)
    ));
    let system = Actor::System {
        component: "engine".into(),
    };
    assert!(TrustedInput::from_owner_note("gate-note:g@1", NOTE, &system).is_none());

    // A tainted checker's reasons.
    let p = project();
    let draft = texts(&["version one", "version two"]);
    let reasons_json = format!(r#"{{"reasons":["{REASONS}"]}}"#);
    let check = texts(&[reasons_json.as_str(), r#"{"reasons":[]}"#]);
    let steps = vec![
        with_prompt(step("draft", draft.clone()), seat_prompt()),
        poisoned(looping("check", check, "draft", 1)),
    ];
    let outcome = run_plan(
        &p.store,
        owner(),
        "manual",
        &plan(steps),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let again = j.attempt("draft", 2);
    assert_eq!(
        again.taint,
        Taint::Tainted,
        "the reasons tainted the target"
    );
    let rows = j.parts_of(&again.id);
    let reasons = rows
        .iter()
        .find(|r| r.label == "loop-back:check@1")
        .expect("the part");
    assert_eq!(
        (reasons.slot, reasons.kind, reasons.taint),
        (PromptSlot::Untrusted, PromptKind::Untrusted, Taint::Tainted)
    );
    let request = draft.requests()[1].clone();
    let user = user_parts(&request);
    assert!(fenced(&user, "loop-back:check@1").contains(REASONS));
    assert!(no_trusted(&user), "{user:#?}");
    assert!(
        carriers(&request, REASONS)
            .iter()
            .all(|c| matches!(c, Part::Untrusted(_)))
    );
    assert!(request.system.iter().all(|i| !i.text().contains(REASONS)));
    assert_the_wall(&j);

    // A Human's note on a tainted producer.
    let p = project();
    let draft = texts(&["version one", "version two"]);
    let gated = plan(vec![
        poisoned(with_prompt(step("draft", draft.clone()), seat_prompt())),
        gate_max("approve", never(), 2),
    ]);
    let waiting = run_plan(
        &p.store,
        owner(),
        "manual",
        &gated,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    p.store
        .writer()
        .resolve_inbox_item(
            HumanActor::new("owner", REVIEWER_HAT),
            waiting.item.as_deref().unwrap(),
            Decision::Return,
            Some(NOTE.to_string()),
        )
        .unwrap();
    continue_run(&p.store, &waiting.run_id, &gated, CancellationToken::new())
        .await
        .unwrap();
    let j = journal(&p.db);
    let again = j.attempt("draft", 2);
    assert_eq!(again.taint, Taint::Tainted, "a note never clears taint");
    let note = j
        .parts_of(&again.id)
        .into_iter()
        .find(|r| r.label == "gate-note:approve@1")
        .expect("the note part");
    assert_eq!((note.kind, note.taint), (PromptKind::Trusted, Taint::Clean));
    let request = draft.requests()[1].clone();
    assert!(request.system.iter().all(|i| !i.text().contains(NOTE)));
    assert_eq!(carriers(&request, NOTE).len(), 1);
    assert!(matches!(&carriers(&request, NOTE)[0], Part::Trusted(_)));
    assert_the_wall(&j);
}

/// A plan input is data: its canary is in no system part, in no journal
/// row of a config slot, and in the one fenced user part only; the row
/// that journals it is `untrusted` and tainted, and it tainted the attempt.
#[tokio::test]
async fn plan_inputs_never_become_system_text() {
    let p = project();
    let llm = texts(&["done"]);
    let write = poisoned(with_prompt(step("write", llm.clone()), seat_prompt()));

    run_plan(
        &p.store,
        owner(),
        "manual",
        &plan(vec![write]),
        CancellationToken::new(),
    )
    .await
    .unwrap();

    let request = llm.requests()[0].clone();
    assert!(
        request.system.iter().all(|i| !i.text().contains(POISON)),
        "never system text: {:?}",
        request.system
    );
    let user = user_parts(&request);
    assert_eq!(fenced(&user, POISON_LABEL), POISON);
    assert!(
        !user
            .iter()
            .any(|p| matches!(p, Part::Instruction(i) if i.text().contains(POISON))),
        "never an instruction: {user:#?}"
    );
    let j = journal(&p.db);
    let id = j.attempt("write", 1).id;
    assert_eq!(j.attempt("write", 1).taint, Taint::Tainted);
    for row in j.parts_of(&id) {
        let holds = row.text.as_deref().is_some_and(|t| t.contains(POISON));
        assert_eq!(holds, row.slot == PromptSlot::Untrusted, "{row:?}");
        if holds {
            assert_eq!(
                (row.kind, row.taint),
                (PromptKind::Untrusted, Taint::Tainted)
            );
        }
    }
    assert_the_wall(&j);
}
