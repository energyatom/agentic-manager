//! Trust test (AD-10, AD-27, AD-36; epic 3 story 5, matrix rows 18, 21 and
//! 24, Question 2 option (a)): a schema repair neither adds nor clears
//! taint, and the validation words never carry the model's text.
//!
//! - **The invalid reply** is replayed to the model as its own words
//!   (`Part::AssistantText`, the assistant role), not wrapped as fenced
//!   `UntrustedData`: a step that read nothing untrusted stays clean after a
//!   repair, and so does what depends on it; a step that did stays
//!   tainted. The repair instruction is the schema's, config text, an
//!   `Instruction` never tainted.
//! - **Words never echo:** a string value, an unexpected key's name and a
//!   value outside an enum, planted as canaries in both invalid messages,
//!   appear in neither the Failure item, the run's reason, the audit log
//!   nor the repair instruction (the first invalid message is replayed to
//!   the model by design; the second is stored nowhere).
//! - **A repair never clears taint:** the output of a repaired, tainted
//!   step is tainted at rest and fenced for the next step; a clean one's is
//!   a trusted input.
//!
//! FakeLlm only. Written before the repair exists.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

use am_domain::{Instruction, Part, Role, Taint};
use am_engine::run::run_plan;
use am_engine::schema::OutputSchema;
use am_store::{PromptKind, PromptSlot};
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

const FIRST_BAD: &str = "I think the line is: hello";
const GOOD: &str = r#"{"line":"hello"}"#;

async fn run(p: &Project, steps: Vec<am_engine::run::StepPlan>) -> am_engine::run::RunOutcome {
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

/// Rows 18 and 24, Question 2: on a clean step the invalid reply is in the
/// second request as assistant text and nowhere else, there is no fence
/// and no `UntrustedData` in it, the journal's reply row is an `assistant`
/// row, clean, and the step and its output stay clean: the next step gets
/// the repaired output as a trusted input.
#[tokio::test]
async fn the_invalid_reply_is_replayed_as_assistant_text_and_taints_nothing() {
    let p = project();
    let write = texts(&[FIRST_BAD, GOOD]);
    let after = texts(&["done"]);
    let steps = vec![
        with_schema(step("write", write.clone()), line_schema()),
        step("after", after.clone()),
    ];

    let outcome = run(&p, steps).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let second = write.requests()[1].clone();
    let carrying = carriers(&second, FIRST_BAD);
    assert_eq!(carrying.len(), 1, "once: {carrying:#?}");
    assert!(
        matches!(&carrying[0], Part::AssistantText(_)),
        "{carrying:#?}"
    );
    assert!(
        second
            .messages
            .iter()
            .flat_map(|m| m.parts.iter())
            .all(|p| !matches!(p, Part::Untrusted(_) | Part::Trusted(_))),
        "no fence and no trusted block in a repair: {second:#?}"
    );
    let assistants: Vec<_> = second
        .messages
        .iter()
        .filter(|m| m.role == Role::Assistant)
        .collect();
    assert_eq!(assistants.len(), 1);
    assert!(second.system.iter().all(|i| !i.text().contains(FIRST_BAD)));

    let j = journal(&p.db);
    let row = j.attempt("write", 1);
    assert_eq!(row.taint, Taint::Clean, "a repair adds no taint");
    let own: Vec<_> = j
        .parts_of(&row.id)
        .into_iter()
        .filter(|p| p.call_seq == 2)
        .collect();
    assert_eq!(
        (own[0].slot, own[0].kind, own[0].taint),
        (PromptSlot::Reply, PromptKind::Assistant, Taint::Clean)
    );
    assert_eq!(own[0].text.as_deref(), Some(FIRST_BAD));
    assert_eq!(
        (own[1].slot, own[1].kind, own[1].taint),
        (PromptSlot::Repair, PromptKind::Instruction, Taint::Clean)
    );
    assert_eq!(j.attempt("after", 1).taint, Taint::Clean);
    let next = user_parts(&after.requests()[0]);
    assert_eq!(
        trusted(&next, "step:write@1").text(),
        GOOD,
        "trusted, not fenced"
    );
}

/// Row 24: a repair never clears taint. A tainted step (a plan input)
/// repaired once stays tainted: its repaired output is tainted at rest and
/// the next step receives it fenced under the same label, never trusted;
/// the reply row is journaled tainted and the instruction row clean.
#[tokio::test]
async fn a_repair_never_clears_taint() {
    let p = project();
    let write = texts(&[FIRST_BAD, GOOD]);
    let after = texts(&["done"]);
    let steps = vec![
        poisoned(with_schema(step("write", write.clone()), line_schema())),
        step("after", after.clone()),
    ];

    let outcome = run(&p, steps).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let row = j.attempt("write", 1);
    assert_eq!(row.taint, Taint::Tainted);
    assert_eq!(row.output.as_deref(), Some(GOOD));
    let own: Vec<_> = j
        .parts_of(&row.id)
        .into_iter()
        .filter(|p| p.call_seq == 2)
        .collect();
    assert_eq!(
        (own[0].kind, own[0].taint),
        (PromptKind::Assistant, Taint::Tainted)
    );
    assert_eq!(
        (own[1].kind, own[1].taint),
        (PromptKind::Instruction, Taint::Clean)
    );
    assert_eq!(
        j.attempt("after", 1).taint,
        Taint::Tainted,
        "taint crosses the step"
    );
    let next = user_parts(&after.requests()[0]);
    assert_eq!(fenced(&next, "step:write@1"), GOOD, "fenced, not trusted");
    assert!(no_trusted(&next), "{next:#?}");
    // The second request carries the plan input's fence exactly as the
    // first did: the repair added none and removed none.
    let fences = |r: &am_domain::ChatRequest| {
        r.messages
            .iter()
            .flat_map(|m| m.parts.iter())
            .filter(|p| matches!(p, Part::Untrusted(_)))
            .count()
    };
    let requests = write.requests();
    assert_eq!(fences(&requests[0]), 1);
    assert_eq!(fences(&requests[1]), 1);
}

/// Row 21: the words never echo the model's text. Both invalid messages
/// carry a canary as a string value, as an unexpected key's name and as a
/// value outside an enum; the step fails after its one repair. The Failure
/// item, the run's reason, the audit log and the repair instruction (the
/// request's and the journal's) hold none of the canaries; the second
/// message is stored nowhere (no output, no part); the words still name the
/// step, the schema and the schema's own field names.
#[tokio::test]
async fn validation_words_hold_no_canary_from_the_message() {
    let schema = OutputSchema::new(
        "choice",
        json!({
            "type": "object",
            "required": ["kind", "count"],
            "additionalProperties": false,
            "properties": {
                "kind": { "enum": ["red", "green"] },
                "count": { "type": "integer" }
            }
        }),
        Instruction::authored(
            "Your last message did not match the output schema `choice`. Reply again with one JSON object that does.",
        ),
    );
    let first = r#"{"kind":"canary-enum-1a2b","count":"canary-str-1a2b","canary-key-1a2b":1}"#;
    let second = r#"{"kind":"canary-enum-9z8y","count":"canary-str-9z8y","canary-key-9z8y":2}"#;
    let p = project();
    let llm = texts(&[first, second, GOOD]);

    let outcome = run(&p, vec![with_schema(step("write", llm.clone()), schema)]).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    assert_eq!(llm.calls(), 2);
    let reason = outcome.reason.clone().unwrap_or_default();
    let j = journal(&p.db);
    let item = &j.items[0];
    let words = format!("{reason}\n{}\n{}", item.title, item.body);
    for canary in ["canary-enum", "canary-str", "canary-key", "1a2b", "9z8y"] {
        assert!(!words.contains(canary), "{canary} in the words: {words}");
    }
    for word in ["write", "choice", "kind", "count"] {
        assert!(words.contains(word), "`{word}` in the words: {words}");
    }
    let export = serde_json::to_string(&j.audit).unwrap();
    for canary in ["canary-enum", "canary-str", "canary-key"] {
        assert!(!export.contains(canary), "{canary} in the audit log");
    }
    // The repair instruction is the schema's own text.
    let sent = llm.requests()[1].clone();
    let instruction = match &sent.messages.last().unwrap().parts[..] {
        [Part::Instruction(i)] => i.text().to_string(),
        other => panic!("the repair instruction last: {other:?}"),
    };
    assert!(!instruction.contains("canary"), "{instruction}");
    let row = j.attempt("write", 1);
    for part in j.parts_of(&row.id) {
        if part.slot == PromptSlot::Repair {
            assert!(!part.text.as_deref().unwrap_or_default().contains("canary"));
        }
    }
    // The second invalid message is stored nowhere.
    assert_eq!(row.output, None);
    for part in j.parts_of(&row.id) {
        assert!(
            !part.text.as_deref().unwrap_or_default().contains("9z8y"),
            "{part:?}"
        );
    }
    assert!(!format!("{:?}", j.items).contains("9z8y"));
}
