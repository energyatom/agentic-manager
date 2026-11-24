//! Epic 3 story 5, AD-36 at the engine: "one repair, then fail" (matrix
//! rows 16 to 19, 22 to 28). A step that declares an `output_schema` has
//! its final message (a turn with no tool call) checked.
//!
//! - **Valid first time:** one call; the output is stored as written.
//! - **Invalid once:** a second call, `purpose = 'repair'`, an ordinary
//!   model call (reserved, settled, one ledger row, counted by the
//!   iteration cap). Its request is the same system and first user message,
//!   then the invalid reply as `Part::AssistantText` in the assistant role,
//!   then the schema's repair `Instruction` in the user role: no fence, no
//!   new `UntrustedData`. The repair call's parts are journaled; the step's
//!   output is the second message.
//! - **Invalid twice:** exactly two calls, no third; the step and the run
//!   `failed`; one Failure item whose words name the step, the schema id
//!   and each failing place and rule (never the model's text); the invalid
//!   texts are not the step's output.
//! - **No turn left, no repair:** the iteration cap leaves none; the
//!   failure says so.
//! - **The check comes first:** a loop-back step's reasons are read only
//!   from a message that passed the schema.
//! - **A bad schema writes nothing:** a schema that cannot be compiled
//!   (a remote or file `$ref`, a pattern the linear-time engine refuses, an
//!   unknown type) refuses the run before any row is written.
//!
//! FakeLlm only; no network. The taint and words rules of the repair are
//! `tests/trust/taint_prompt_repair.rs`. Written before any of it exists.
#![forbid(unsafe_code)]

#[path = "support/mod.rs"]
mod support;

use am_domain::{Instruction, Message, Part, Role, Taint};
use am_engine::run::{Limits, RunError, run_plan};
use am_engine::schema::OutputSchema;
use am_store::{CallPurpose, PromptKind, PromptSlot};
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

const GOOD: &str = r#"{"line":"hello"}"#;
const SECOND_GOOD: &str = "{\"line\": \"second\"}\n";
const PROSE: &str = "I would say hello to you";
const WRONG_TYPE: &str = r#"{"line": 7}"#;

fn schema_step(
    key: &str,
    provider: std::sync::Arc<dyn am_domain::LlmProvider>,
) -> am_engine::run::StepPlan {
    with_schema(step(key, provider), line_schema())
}

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

/// Row 16: a final message that matches is the step's output as written
/// (its whitespace too); one call, one ledger row, no repair.
#[tokio::test]
async fn a_matching_message_is_not_repaired() {
    let p = project();
    let message = format!("  {GOOD}\n");
    let llm = texts(&[message.as_str(), "never"]);

    let outcome = run(&p, vec![schema_step("write", llm.clone())]).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert_eq!(llm.calls(), 1);
    let j = journal(&p.db);
    let row = j.attempt("write", 1);
    assert_eq!(row.status, "succeeded");
    assert_eq!(
        row.output.as_deref(),
        Some(message.as_str()),
        "stored as written"
    );
    assert_eq!(j.calls_of(&row.id).len(), 1);
    assert!(
        j.calls_of(&row.id)
            .iter()
            .all(|c| c.purpose == CallPurpose::Turn)
    );
    assert_eq!(j.ledger.len(), 1);
    assert!(
        j.parts_of(&row.id).iter().all(|p| p.call_seq == 0),
        "no repair rows"
    );
}

/// Rows 17 and 23: an invalid first message is repaired once. Two calls,
/// the second a `repair`; two ledger reservations, each settled; no other
/// reservation path; the audit entries name the purposes; the step
/// succeeded with the second message as its output; the repair call's two
/// parts are journaled under its own call number.
#[tokio::test]
async fn an_invalid_message_is_repaired_once_and_marked() {
    let p = project();
    let llm = texts(&[PROSE, SECOND_GOOD, "never"]);

    let outcome = run(&p, vec![schema_step("write", llm.clone())]).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert_eq!(llm.calls(), 2);
    let j = journal(&p.db);
    let row = j.attempt("write", 1);
    assert_eq!(row.status, "succeeded");
    assert_eq!(
        row.output.as_deref(),
        Some(SECOND_GOOD),
        "the second message"
    );
    let calls = j.calls_of(&row.id);
    assert_eq!(
        calls
            .iter()
            .map(|c| (c.call_seq, c.purpose))
            .collect::<Vec<_>>(),
        [(1, CallPurpose::Turn), (2, CallPurpose::Repair)]
    );
    assert!(calls.iter().all(|c| c.settled), "{calls:#?}");
    assert_eq!(j.ledger.len(), 2, "one reservation per call");
    assert!(
        j.ledger.iter().all(|l| l.state == "settled"),
        "{:#?}",
        j.ledger
    );
    let reserved = j.entries("model_call.reserved");
    assert_eq!(
        reserved
            .iter()
            .map(|e| e["args"]["purpose"].clone())
            .collect::<Vec<_>>(),
        [json!("turn"), json!("repair")]
    );
    assert_eq!(
        j.entries("model_call.settled+step.finished").len() + j.entries("model_call.settled").len(),
        2,
        "each call is settled: {:?}",
        j.actions()
    );

    // The repair call's parts: the reply, then the instruction.
    let own: Vec<_> = j
        .parts_of(&row.id)
        .into_iter()
        .filter(|p| p.call_seq == 2)
        .collect();
    assert_eq!(own.len(), 2, "{own:#?}");
    assert_eq!(
        (
            own[0].ord,
            own[0].slot,
            own[0].kind,
            own[0].label.as_str(),
            own[0].text.as_deref()
        ),
        (
            1,
            PromptSlot::Reply,
            PromptKind::Assistant,
            "reply:write@1",
            Some(PROSE)
        )
    );
    assert_eq!(
        (
            own[1].ord,
            own[1].slot,
            own[1].kind,
            own[1].label.as_str(),
            own[1].text.as_deref()
        ),
        (
            2,
            PromptSlot::Repair,
            PromptKind::Instruction,
            "repair",
            Some(REPAIR)
        )
    );
}

/// Row 18: the repair request is the first request's system and first
/// user message, then the invalid reply as assistant text, then the
/// schema's repair instruction in the user role. No fence and no new
/// untrusted part; the step's taint is unchanged (clean).
#[tokio::test]
async fn the_repair_prompt_replays_the_reply_and_adds_the_schema_instruction() {
    let p = project();
    let llm = texts(&[PROSE, GOOD]);

    run(&p, vec![schema_step("write", llm.clone())]).await;

    let requests = llm.requests();
    assert_eq!(requests.len(), 2);
    let (first, second) = (&requests[0], &requests[1]);
    assert_eq!(second.system, first.system, "the same system parts");
    assert_eq!(second.model, first.model);
    assert_eq!(second.messages.len(), 3, "{:#?}", second.messages);
    assert_eq!(
        second.messages[0], first.messages[0],
        "the same first user message"
    );
    assert_eq!(
        second.messages[1],
        Message {
            role: Role::Assistant,
            parts: vec![Part::AssistantText(PROSE.to_string())],
        }
    );
    assert_eq!(
        second.messages[2],
        Message {
            role: Role::User,
            parts: vec![Part::Instruction(Instruction::authored(REPAIR))],
        }
    );
    let fences = |r: &am_domain::ChatRequest| {
        r.messages
            .iter()
            .flat_map(|m| m.parts.iter())
            .filter(|p| matches!(p, Part::Untrusted(_)))
            .count()
    };
    assert_eq!(fences(second), fences(first), "no new UntrustedData");
    assert_eq!(fences(second), 0);
    let j = journal(&p.db);
    assert_eq!(
        j.attempt("write", 1).taint,
        Taint::Clean,
        "the taint is unchanged"
    );
}

/// Row 19: both messages invalid. Exactly two calls; the step and the run
/// `failed`; one Failure item whose words name the step, the schema's id
/// and what failed (a rule and a place from the schema); the step has no
/// output (an invalid message is no output); the run's reason is the same
/// words.
#[tokio::test]
async fn a_second_invalid_message_fails_with_a_failure_item_in_words() {
    let p = project();
    let llm = texts(&[PROSE, WRONG_TYPE, GOOD]);

    let outcome = run(&p, vec![schema_step("write", llm.clone())]).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let reason = outcome.reason.clone().unwrap_or_default();
    for word in ["write", "line", "output schema", "string"] {
        assert!(reason.contains(word), "`{word}` in the reason: {reason}");
    }
    let j = journal(&p.db);
    assert_eq!(j.runs[0].status, "failed");
    let row = j.attempt("write", 1);
    assert_eq!(row.status, "failed");
    assert_eq!(row.output, None, "an invalid message is not an output");
    assert_eq!(j.items.len(), 1, "{:#?}", j.items);
    let item = &j.items[0];
    assert_eq!(item.kind, am_domain::InboxKind::Failure);
    assert_eq!(item.step_run_id, row.id);
    assert_eq!(outcome.item.as_deref(), Some(item.id.as_str()));
    let words = format!("{} {}", item.title, item.body);
    for word in ["write", "line", "string"] {
        assert!(words.contains(word), "`{word}` in the item: {words}");
    }
    assert!(
        !words.contains("would say hello"),
        "never the model's text: {words}"
    );
    assert_eq!(item.taint, Taint::Clean);
}

/// Row 19: no third call. With a valid third message scripted, the
/// provider is still called twice; the journal holds two calls and two
/// ledger rows; the attempt's own prompt and the one repair's parts are
/// its only parts.
#[tokio::test]
async fn no_third_call_is_made() {
    let p = project();
    let llm = texts(&[PROSE, PROSE, GOOD]);

    let outcome = run(&p, vec![schema_step("write", llm.clone())]).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    assert_eq!(llm.calls(), 2, "one repair, then fail");
    let j = journal(&p.db);
    let id = j.attempt("write", 1).id;
    assert_eq!(j.calls_of(&id).len(), 2);
    assert_eq!(j.ledger.len(), 2);
    assert!(j.calls_of(&id).iter().all(|c| c.settled));
    let seqs: Vec<i64> = j.parts_of(&id).iter().map(|p| p.call_seq).collect();
    assert!(seqs.iter().all(|s| *s == 0 || *s == 2), "{seqs:?}");
    assert_eq!(seqs.iter().filter(|s| **s == 2).count(), 2);
}

/// Row 22: the iteration cap counts the repair. With `limits.iterations:
/// 1` the first invalid message leaves no turn to repair it: one call, no
/// repair, the step fails with one Failure item that says the message did
/// not match the schema and the cap left no turn to repair it.
#[tokio::test]
async fn no_turn_left_means_no_repair() {
    let p = project();
    let llm = texts(&[PROSE, GOOD]);
    let limited = am_engine::run::StepPlan {
        limits: Limits {
            iterations: 1,
            ..Limits::default()
        },
        ..schema_step("write", llm.clone())
    };

    let outcome = run(&p, vec![limited]).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    assert_eq!(llm.calls(), 1, "no repair call");
    let j = journal(&p.db);
    let row = j.attempt("write", 1);
    assert_eq!(row.status, "failed");
    assert_eq!(j.calls_of(&row.id).len(), 1);
    assert_eq!(j.items.len(), 1);
    let words = format!(
        "{} {} {}",
        outcome.reason.clone().unwrap_or_default(),
        j.items[0].title,
        j.items[0].body
    );
    for word in ["output schema", "iterations", "repair"] {
        assert!(words.contains(word), "`{word}`: {words}");
    }
    // R14 (review round 1): the item says it is about the schema, not about
    // a cap in general.
    assert!(
        j.items[0].title.contains("output schema"),
        "the title: {}",
        j.items[0].title
    );
    assert!(
        j.items[0].title.contains("did not match"),
        "the title: {}",
        j.items[0].title
    );
    assert!(names_number(&words, 1), "{words}");
    assert!(!words.contains("would say hello"), "{words}");
}

/// Row 25: the schema is checked before the loop-back reads the reasons.
/// A message whose `reasons` would loop the run back but that does not
/// match the schema is repaired first; the loop-back's reasons are the
/// repaired message's, never the invalid one's. A step whose messages are
/// both invalid fails without looping back.
#[tokio::test]
async fn the_schema_is_checked_before_the_loop_back() {
    let reasons_schema = OutputSchema::new(
        "verdict",
        json!({
            "type": "object",
            "required": ["reasons", "verdict"],
            "properties": {
                "reasons": { "type": "array", "items": { "type": "string" } },
                "verdict": { "type": "string" }
            }
        }),
        Instruction::authored("Reply again with an object holding `reasons` and `verdict`."),
    );
    let invalid = r#"{"reasons":["canary-from-the-invalid-message"]}"#;
    let valid = r#"{"reasons":["tighten the opening"],"verdict":"fix"}"#;
    let done = r#"{"reasons":[],"verdict":"fine"}"#;

    let p = project();
    let draft = texts(&["version one", "version two"]);
    let check = texts(&[invalid, valid, done]);
    let steps = vec![
        step("draft", draft.clone()),
        with_schema(
            looping("check", check.clone(), "draft", 1),
            reasons_schema.clone(),
        ),
    ];

    let outcome = run(&p, steps).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let first_check = j.attempt("check", 1);
    assert_eq!(first_check.status, "looped_back");
    assert_eq!(
        first_check.output.as_deref(),
        Some(valid),
        "the repaired message"
    );
    assert_eq!(j.calls_of(&first_check.id).len(), 2, "turn, then repair");
    let again = user_parts(&draft.requests()[1]);
    let reasons = trusted(&again, "loop-back:check@1").text().to_string();
    assert!(reasons.contains("tighten the opening"), "{reasons}");
    assert!(
        !reasons.contains("canary-from-the-invalid-message"),
        "{reasons}"
    );
    assert_eq!(j.attempt("check", 2).status, "succeeded");

    // Invalid twice: no loop-back, the step fails.
    let p = project();
    let draft = texts(&["version one", "version two"]);
    let check = texts(&[invalid, invalid]);
    let steps = vec![
        step("draft", draft.clone()),
        with_schema(looping("check", check, "draft", 1), reasons_schema),
    ];

    let outcome = run(&p, steps).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let j = journal(&p.db);
    assert_eq!(
        j.attempts("draft").len(),
        1,
        "no loop-back: {:?}",
        j.shape()
    );
    assert_eq!(j.attempt("check", 1).status, "failed");
    assert_eq!(draft.calls(), 1);
}

/// Row 24: a repair neither adds nor clears taint. On a tainted attempt
/// (a plan input) the repair's reply row is journaled tainted and the
/// attempt stays tainted; on a clean attempt the same rows are clean; in
/// both the repair instruction is an instruction, never tainted.
#[tokio::test]
async fn a_repair_on_a_tainted_step_stays_tainted_and_a_clean_one_stays_clean() {
    for tainted in [false, true] {
        let p = project();
        let llm = texts(&[PROSE, GOOD]);
        let mut write = schema_step("write", llm.clone());
        if tainted {
            write = poisoned(write);
        }

        let outcome = run(&p, vec![write]).await;

        assert_eq!(
            outcome.status, "succeeded",
            "tainted: {tainted}: {outcome:?}"
        );
        let j = journal(&p.db);
        let row = j.attempt("write", 1);
        let want = if tainted {
            Taint::Tainted
        } else {
            Taint::Clean
        };
        assert_eq!(row.taint, want, "the attempt");
        let own: Vec<_> = j
            .parts_of(&row.id)
            .into_iter()
            .filter(|p| p.call_seq == 2)
            .collect();
        assert_eq!(own.len(), 2, "{own:#?}");
        assert_eq!((own[0].kind, own[0].taint), (PromptKind::Assistant, want));
        assert_eq!(
            (own[1].kind, own[1].taint),
            (PromptKind::Instruction, Taint::Clean),
            "an instruction is never tainted"
        );
        for call in j.calls_of(&row.id) {
            assert!(call.settled);
        }
        let reserved = j.entries("model_call.reserved");
        assert_eq!(reserved.len(), 2);
        for entry in reserved {
            let flag = &entry["args"]["taint"];
            assert_eq!(audited_tainted(flag), tainted, "{entry}");
        }
    }
}

/// Rows 26 and 27: a step whose schema cannot be compiled (a remote or a
/// file `$ref`, a look-around pattern, an unknown type) refuses the run
/// before any row is written, naming the step and the schema's id: no run,
/// no step, no audit entry, no model call, even when the bad step is the
/// second of the workflow.
#[tokio::test]
async fn a_run_with_a_bad_schema_writes_no_row() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("elsewhere.json");
    std::fs::write(&file, r#"{"type":"integer"}"#).unwrap();
    let cases = [
        (
            "a remote $ref",
            json!({"type": "object", "properties": {"line": {"$ref": "https://example.org/line.json"}}}),
        ),
        (
            "a file $ref",
            json!({"type": "object", "properties": {"line": {"$ref": file.display().to_string()}}}),
        ),
        (
            "a look-around pattern",
            json!({"type": "string", "pattern": "(?=a)b"}),
        ),
        ("an unknown type", json!({"type": "strng"})),
    ];
    for (what, schema) in cases {
        let p = project();
        let (first, second) = (texts(&[GOOD]), texts(&[GOOD]));
        let bad = OutputSchema::new("broken", schema, Instruction::authored("Reply again."));
        let steps = vec![
            step("one", first.clone()),
            with_schema(step("write", second.clone()), bad),
        ];

        let refused = run_plan(
            &p.store,
            owner(),
            "manual",
            &plan(steps),
            CancellationToken::new(),
        )
        .await;

        match refused {
            Err(RunError::Schema { step, why }) => {
                assert_eq!(step, "write", "{what}");
                assert!(why.contains("broken"), "{what}: names the schema: {why}");
            }
            other => panic!("{what}: RunError::Schema expected: {other:?}"),
        }
        let j = journal(&p.db);
        assert!(
            j.runs.is_empty() && j.steps.is_empty() && j.calls.is_empty(),
            "{what}"
        );
        assert!(j.audit.is_empty(), "{what}: {:#?}", j.audit);
        assert_eq!(first.calls() + second.calls(), 0, "{what}: no model asked");
    }
}

/// The owner's audit-hash decision (2026-11-24): across a repaired run the
/// only hash argument is the existing `output_sha256`, each value
/// `audit::sha256_hex` of text the journal stores (the step's output, or a
/// stored part); no new `*_sha256` key appears, and nothing hashes the
/// invalid reply or the validation words. A failed run's finish hashes
/// nothing.
#[tokio::test]
async fn a_repaired_run_audits_only_the_helpers_output_hash() {
    for (messages, status) in [
        ([PROSE, SECOND_GOOD], "succeeded"),
        ([PROSE, PROSE], "failed"),
    ] {
        let p = project();

        let outcome = run(&p, vec![schema_step("write", texts(&messages))]).await;

        assert_eq!(outcome.status, status, "{outcome:?}");
        let j = journal(&p.db);
        let mut stored: Vec<String> = j
            .steps
            .iter()
            .filter_map(|s| s.output.clone())
            .chain(j.prompt.values().flatten().filter_map(|p| p.text.clone()))
            .collect();
        stored.push(String::new());
        let mut hashes = 0;
        for entry in &j.audit {
            let Some(args) = entry["args"].as_object() else {
                continue;
            };
            for (key, value) in args.iter().filter(|(k, _)| k.ends_with("_sha256")) {
                let Some(hash) = value.as_str() else { continue };
                assert_eq!(key, "output_sha256", "no new hash key: {entry}");
                assert!(
                    stored
                        .iter()
                        .any(|t| am_store::audit::sha256_hex(t) == hash),
                    "{entry}: not the helper's hash of stored text"
                );
                hashes += 1;
            }
        }
        if status == "succeeded" {
            assert!(hashes >= 1, "the finish hashes the output");
        }
        let export = serde_json::to_string(&j.audit).unwrap();
        assert!(!export.contains("would say hello"), "{export}");
    }
}
