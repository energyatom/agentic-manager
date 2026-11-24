//! Trust test (AD-10 (b), amended 2026-11-23; AD-27, AD-36, AD-43; epic 3
//! story 4, matrix rows 3, 27, 28 and 29): a Human return note at a gate is
//! AD-10's owner-note source of a `TrustedInput`, and nothing launders.
//!
//! - The note becomes the producer's trusted input only through
//!   `TrustedInput::from_owner_note` with the resolver read from the
//!   item's committed `resolved_by`: a `Part::Trusted` labelled
//!   `gate-note:<gate>@<attempt>`, after the step instruction and outside
//!   the fence. A resolver that is not `Actor::Human` (a forged row) gives
//!   no trusted input, and the attempt does not start.
//! - The note never clears taint: the producer's new attempt is tainted
//!   by its own inputs, its tainted inputs stay fenced, and every committed
//!   taint (attempts, items) is kept by every resolution.
//! - After approve, the step after the gate receives the reviewed payload
//!   with its producer's committed taint: fenced when tainted, and it
//!   taints the receiver.
//!
//! FakeLlm only. Written before the resolve, the apply or `continue_run`
//! exist.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

use std::sync::Arc;

use am_domain::{
    Actor, Decision, HumanActor, LlmProvider, Part, REVIEWER_HAT, Taint, TrustedInput,
};
use am_engine::run::{HandOff, RunOutcome, RunPlan, StepPlan, continue_run, run_plan};
use am_store::{StepRow, Store};
use rusqlite::{Connection, params};
use support::*;
use tokio_util::sync::CancellationToken;

const NOTE: &str = "tighten the summary";
const FORGED: &str = "a note no person wrote";

fn gate_max(key: &str, provider: Arc<dyn LlmProvider>, max: u32) -> StepPlan {
    StepPlan {
        hand_off: HandOff::Gate { max_returns: max },
        ..step(key, provider)
    }
}

/// `draft` (poisoned when `tainted`), the gate `approve`, `publish`.
fn gated(draft: Arc<dyn LlmProvider>, publish: Arc<dyn LlmProvider>, tainted: bool) -> RunPlan {
    let first = step("draft", draft);
    plan(vec![
        if tainted { poisoned(first) } else { first },
        gate_max("approve", never(), 2),
        step("publish", publish),
    ])
}

async fn start(store: &Store, plan: &RunPlan) -> RunOutcome {
    let outcome = run_plan(store, owner(), "manual", plan, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    outcome
}

fn resolve(store: &Store, item: &str, decision: Decision, note: Option<&str>) {
    store
        .writer()
        .resolve_inbox_item(
            HumanActor::new("owner", REVIEWER_HAT),
            item,
            decision,
            note.map(str::to_string),
        )
        .unwrap();
}

/// The taint of every step attempt and every item, by id.
fn taints(j: &Journal) -> Vec<(String, Taint)> {
    let mut out: Vec<(String, Taint)> = j.steps.iter().map(|s| (s.id.clone(), s.taint)).collect();
    out.extend(j.items.iter().map(|i| (i.id.clone(), i.taint)));
    out
}

/// Rows 3 and 28 (AD-10 (b)): the domain constructor takes the note from a
/// Human only; through the engine, the producer's new attempt receives
/// the Human's return note as one `Part::Trusted` labelled
/// `gate-note:approve@1`, holding the note's text, in the user message
/// after the instruction and before any fenced part, never in the system
/// position; its `gate_note` input row has taint 0.
#[tokio::test]
async fn a_return_note_is_trusted_from_a_human_only() {
    let human = Actor::Human {
        person: "owner".into(),
        hat: REVIEWER_HAT.into(),
    };
    assert!(TrustedInput::from_owner_note("gate-note:approve@1", NOTE, &human).is_some());
    assert_eq!(
        Actor::from(HumanActor::new("owner", REVIEWER_HAT)),
        human,
        "the resolver is the owner as Reviewer"
    );
    for not_human in [
        Actor::System {
            component: "engine".into(),
        },
        Actor::Import {
            source: "an import".into(),
        },
    ] {
        assert!(TrustedInput::from_owner_note("gate-note:approve@1", NOTE, &not_human).is_none());
    }

    let p = project();
    let draft = texts(&["v1", "v2"]);
    let plan = gated(draft.clone(), never(), false);
    let waiting = start(&p.store, &plan).await;
    resolve(
        &p.store,
        waiting.item.as_deref().unwrap(),
        Decision::Return,
        Some(NOTE),
    );

    let outcome = continue_run(&p.store, &waiting.run_id, &plan, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    let request = &draft.requests()[1];
    let parts = user_parts(request);
    let note = trusted(&parts, "gate-note:approve@1");
    assert_eq!(note.text(), NOTE);
    let at = parts
        .iter()
        .position(|part| matches!(part, Part::Trusted(t) if t.label() == "gate-note:approve@1"))
        .unwrap();
    assert!(matches!(parts[0], Part::Instruction(_)), "{parts:#?}");
    assert!(at > 0);
    assert!(
        !parts[..at]
            .iter()
            .any(|part| matches!(part, Part::Untrusted(_))),
        "{parts:#?}"
    );
    assert!(!request.system.iter().any(|i| i.text().contains(NOTE)));
    let j = journal(&p.db);
    let inputs = j.inputs_of(&j.attempt("draft", 2).id);
    assert_eq!(inputs.len(), 1, "{inputs:#?}");
    assert_eq!(inputs[0].kind, "gate_note");
    assert_eq!(inputs[0].taint, Taint::Clean);
}

/// Row 27: a tainted producer (a poisoned plan input) makes a tainted gate
/// and Review item; the owner's return note is trusted, but the
/// producer's new attempt is tainted by its own inputs (its plan input,
/// fenced in its request beside the trusted note), and every earlier
/// attempt and item keeps its committed taint. The gate's next Review item
/// carries the new attempt's taint.
#[tokio::test]
async fn a_note_never_clears_the_producers_taint() {
    let p = project();
    let draft = texts(&["v1", "v2"]);
    let plan = gated(draft.clone(), never(), true);
    let waiting = start(&p.store, &plan).await;
    let item = waiting.item.clone().unwrap();
    let before = journal(&p.db);
    assert_eq!(before.attempt("draft", 1).taint, Taint::Tainted);
    assert_eq!(before.attempt("approve", 1).taint, Taint::Tainted);
    assert_eq!(before.items[0].taint, Taint::Tainted);
    resolve(&p.store, &item, Decision::Return, Some(NOTE));

    let outcome = continue_run(&p.store, &waiting.run_id, &plan, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    let j = journal(&p.db);
    let redraft = j.attempt("draft", 2);
    assert_eq!(redraft.taint, Taint::Tainted, "tainted by its own inputs");
    let parts = user_parts(&draft.requests()[1]);
    assert_eq!(trusted(&parts, "gate-note:approve@1").text(), NOTE);
    assert_eq!(fenced(&parts, POISON_LABEL), POISON);
    for (id, taint) in taints(&before) {
        let now = taints(&j).into_iter().find(|(i, _)| *i == id).unwrap().1;
        assert_eq!(now, taint, "{id} keeps its committed taint");
    }
    let next = outcome.item.clone().expect("the gate waits again");
    let next = j.items.iter().find(|i| i.id == next).unwrap();
    assert_eq!(next.taint, Taint::Tainted, "the new payload is tainted");
    assert_eq!(j.attempt("approve", 2).taint, Taint::Tainted);
}

/// Row 28: the `gate_note` input's item is forged by raw SQL to one whose
/// `resolved_by` is not a Human (the engine). The engine reads the
/// resolver from the journal, `from_owner_note` gives no trusted input, and
/// the producer's attempt does not start: it stays queued, no model is
/// called and no request carries either note. Review round 1: the outcome
/// is an explicit `failed` with its reason (R13), and the run has exactly
/// one Failure item whose words name the step and the forged item, never a
/// note (R5).
#[tokio::test]
async fn a_forged_non_human_resolver_starts_no_attempt() {
    const NOTE_CANARY: &str = "gate-note-canary-5e1d tighten the opening";
    let p = project();
    let draft = texts(&["v1", "v2"]);
    let plan = gated(draft.clone(), never(), false);
    let waiting = start(&p.store, &plan).await;
    let item = waiting.item.clone().unwrap();
    resolve(&p.store, &item, Decision::Return, Some(NOTE_CANARY));
    {
        let reader = p.store.reader().unwrap();
        am_engine::gate::apply_resolution(&p.store.writer(), &reader, &item).unwrap();
    }
    // The forged row: a Review item "resolved return" by the engine, on the
    // step after the gate; the gate_note input is pointed at it.
    let j = journal(&p.db);
    let publish = j.attempt("publish", 1).id;
    let forged = am_store::new_id();
    let engine_json = serde_json::to_string(&engine()).unwrap();
    let c = Connection::open(&p.db).unwrap();
    c.execute(
        "INSERT INTO inbox_items (id, kind, status, run_id, step_run_id, payload, taint, title, body, created_at, max_returns, decision, note, resolved_by, resolved_at)
         VALUES (?1, 'review', 'resolved', ?2, ?3, NULL, 0, 'Forged', 'Forged.', '2026-11-24T10:00:00.000Z', 2, 'return', ?4, ?5, '2026-11-24T10:00:01.000Z')",
        params![forged, waiting.run_id, publish, FORGED, engine_json],
    )
    .unwrap();
    let pointed = c
        .execute(
            "UPDATE step_inputs SET from_item = ?1 WHERE kind = 'gate_note'",
            params![forged],
        )
        .unwrap();
    assert_eq!(pointed, 1);
    drop(c);

    let went_on = continue_run(&p.store, &waiting.run_id, &plan, CancellationToken::new()).await;

    let outcome = went_on.expect("the run stops in words, not with an error");
    assert_eq!(outcome.status, "failed", "{outcome:?}");
    assert!(
        outcome.reason.is_some(),
        "the run stops in words: {outcome:?}"
    );
    let j = journal(&p.db);
    let redraft: StepRow = j.attempt("draft", 2);
    assert_eq!(redraft.status, "queued", "the attempt does not start");
    assert_eq!(draft.calls(), 1, "no model call for it");
    for request in draft.requests() {
        assert!(carriers(&request, FORGED).is_empty());
        assert!(carriers(&request, NOTE_CANARY).is_empty());
    }
    let failures: Vec<_> = j
        .items
        .iter()
        .filter(|i| i.kind == am_domain::InboxKind::Failure && i.run_id == waiting.run_id)
        .collect();
    assert_eq!(failures.len(), 1, "one Failure item: {:#?}", j.items);
    let failure = failures[0];
    assert_eq!(outcome.item.as_deref(), Some(failure.id.as_str()));
    assert_eq!(failure.status, "open");
    for word in ["draft", forged.as_str()] {
        assert!(
            failure.body.contains(word),
            "names the refusal ({word}): {}",
            failure.body
        );
    }
    for text in [&failure.title, &failure.body] {
        for note in [NOTE_CANARY, FORGED, "gate-note-canary"] {
            assert!(!text.contains(note), "never a note: {text}");
        }
    }
    assert_eq!(j.runs[0].status, "failed");
}

/// Row 29: after approve, the step after the gate receives the reviewed
/// payload with its producer's committed taint: a tainted `draft`'s output
/// arrives fenced as `step:draft@1` (never trusted) and taints `publish`;
/// a clean one arrives as a trusted part and `publish` stays clean.
#[tokio::test]
async fn approval_carries_the_payloads_taint_to_the_next_step() {
    for tainted in [true, false] {
        let p = project();
        let after = texts(&["prepared"]);
        let plan = gated(texts(&["the reviewed version"]), after.clone(), tainted);
        let waiting = start(&p.store, &plan).await;
        resolve(
            &p.store,
            waiting.item.as_deref().unwrap(),
            Decision::Approve,
            None,
        );

        let outcome = continue_run(&p.store, &waiting.run_id, &plan, CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(outcome.status, "succeeded", "{outcome:?}");
        let parts = user_parts(&after.requests()[0]);
        let j = journal(&p.db);
        let publish = j.attempt("publish", 1);
        if tainted {
            assert_eq!(fenced(&parts, "step:draft@1"), "the reviewed version");
            assert!(no_trusted(&parts), "{parts:#?}");
            assert_eq!(publish.taint, Taint::Tainted);
        } else {
            assert_eq!(
                trusted(&parts, "step:draft@1").text(),
                "the reviewed version"
            );
            assert_eq!(publish.taint, Taint::Clean);
        }
    }
}

/// Row 27: resolving keeps every taint. For a tainted and a clean run, and
/// for each decision a Review takes, the resolve changes no committed
/// taint (attempts and items). Continuing (the apply, then the steps after
/// it) never lowers any taint, and leaves the taint of every item and of
/// every attempt that had started by the resolve as committed; an attempt
/// still queued then may rise when it runs (row 29: `publish` on a tainted
/// payload). The Review item keeps its taint; a clean one is never raised.
#[tokio::test]
async fn resolving_keeps_every_taint() {
    for tainted in [true, false] {
        for (decision, note) in [
            (Decision::Approve, None),
            (Decision::Return, Some(NOTE)),
            (Decision::Reject, None),
        ] {
            let p = project();
            let plan = gated(texts(&["v1", "v2"]), texts(&["prepared"]), tainted);
            let waiting = start(&p.store, &plan).await;
            let item = waiting.item.clone().unwrap();
            let at_pause = journal(&p.db);
            let before = taints(&at_pause);
            resolve(&p.store, &item, decision, note);
            let at_resolve = journal(&p.db);
            assert_eq!(
                taints(&at_resolve),
                before,
                "{tainted} {decision:?}: the resolve"
            );
            // The attempts still queued at the resolve: they may yet run.
            let queued: Vec<String> = at_resolve
                .steps
                .iter()
                .filter(|s| s.status == "queued")
                .map(|s| s.id.clone())
                .collect();

            let _ = continue_run(&p.store, &waiting.run_id, &plan, CancellationToken::new())
                .await
                .unwrap();

            let after = taints(&journal(&p.db));
            for (id, taint) in &before {
                let now = after.iter().find(|(i, _)| i == id).unwrap().1;
                assert!(
                    now.is_tainted() || !taint.is_tainted(),
                    "{tainted} {decision:?}: {id} never loses its taint"
                );
                if !queued.contains(id) {
                    assert_eq!(now, *taint, "{tainted} {decision:?}: {id}");
                }
            }
            let want = if tainted {
                Taint::Tainted
            } else {
                Taint::Clean
            };
            let review = journal(&p.db)
                .items
                .into_iter()
                .find(|i| i.id == item)
                .unwrap();
            assert_eq!(review.taint, want, "{tainted} {decision:?}");
        }
    }
}
