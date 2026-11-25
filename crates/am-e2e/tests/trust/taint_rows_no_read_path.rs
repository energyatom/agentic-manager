//! Trust test (AD-10, AD-21, AD-43; epic 3 story 6's ToolGate slice, owner
//! conditions a and b of the second re-slice, plan matrix row 43; widened
//! by story 3.6c, matrix rows 38 and 47): a row's content reaches a model
//! call only through a step's `row_inputs` slot, and there only fenced and
//! tainting.
//!
//! The ToolGate slice let a step write rows and gave no step a way to read
//! them. Story 3.6c opens exactly one read path: the plan's `row_inputs`
//! slot (tests fill it; 3.15 and 7.2 fill it from config), read at the
//! attempt's start in am-engine's `row_inputs.rs`, the one shipped file
//! outside am-store that `cargo xtask check-deps` lets name a row-content
//! reader (`row_snapshot`); the other readers and records stay in am-store
//! and `rusqlite` in am-store alone (`xtask/tests/check_deps.rs`). This
//! test runs the proof end to end, with a clean writer and with a tainted
//! one:
//!
//! - step A (`collect`) inserts two rows whose data holds canaries;
//! - step B (`revise`, clean, also writing `entries`) updates a canary row
//!   with a canary-free patch;
//! - step C (`close`) has no tools.
//!
//! None of them fills `row_inputs`, so no later request holds a canary in
//! any system, instruction, trusted, untrusted, tool-result or
//! engine-result part (nor anywhere in its debug form): none at all in B's
//! or C's requests, and in A's later turns only in A's own replayed
//! `Part::ToolCall` arguments (the model's own words). Every engine result
//! holds only the row id, the version and its word. B's and C's journaled
//! inputs are only the kinds of stories 3.3 and 3.4; none names a row.
//!
//! Then a later step that names both rows in its slot gets each canary only
//! inside its own row's fence, is tainted, and journals one `row` edge per
//! row naming the after-image it read (3.6c).
//!
//! No network, no real model: `Live` models, whose turns the test makes
//! from the journal (the test reads the row id; no step does).
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

use std::sync::Arc;

use am_domain::rows::{RowStatus, RowWritten};
use am_domain::{ChatRequest, EngineResult, Part, Role, Taint};
use am_engine::run::{RunOutcome, StepPlan, run_plan};
use am_testkit::FakeLlm;
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

/// In the first row's `title`.
const TITLE_CANARY: &str = "canary-row-title-c0a1";
/// In the second row's `extra` (a JSON column) and its `ref`.
const EXTRA_CANARY: &str = "canary-row-extra-c0a2";
const REF_CANARY: &str = "canary-row-ref-c0a3";

const CANARIES: [&str; 3] = [TITLE_CANARY, EXTRA_CANARY, REF_CANARY];

/// The kinds of step input stories 3.3 and 3.4 journal.
const INPUT_KINDS: [&str; 4] = ["step_output", "loop_back", "untrusted", "gate_note"];

struct Ran {
    p: Project,
    outcome: RunOutcome,
    a: Arc<Live>,
    b: Arc<Live>,
    c: Arc<FakeLlm>,
}

/// A, B and C as the module docs say; A is poisoned (tainted from its
/// start) when `tainted_writer`.
async fn three_steps(tainted_writer: bool) -> Ran {
    let p = project();
    let a = Live::new(
        &p.db,
        vec![
            write(insert(json!({ "ref": "r-1", "title": TITLE_CANARY }))),
            write(insert(json!({
                "ref": REF_CANARY,
                "extra": { "note": EXTRA_CANARY },
            }))),
            say("two entries recorded"),
        ],
    );
    let b = Live::new(
        &p.db,
        vec![
            write_with(|r| {
                let first = r
                    .table_rows_of(TABLE)
                    .unwrap()
                    .into_iter()
                    .next()
                    .expect("A's first row");
                update(&first.row_id, first.version, json!({ "state": "done" }))
            }),
            say("one entry revised"),
        ],
    );
    let c = texts(&["closed"]);
    let collect = writing(step("collect", a.clone()));
    let collect = if tainted_writer {
        poisoned(collect)
    } else {
        collect
    };
    let steps: Vec<StepPlan> = vec![
        collect,
        writing(step("revise", b.clone())),
        step("close", c.clone()),
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
    Ran {
        p,
        outcome,
        a,
        b,
        c,
    }
}

/// Every place in `request` a canary stands, as (canary, the part's kind).
fn canaries_in(request: &ChatRequest) -> Vec<(&'static str, String)> {
    let mut found = Vec::new();
    for canary in CANARIES {
        for part in carriers(request, canary) {
            let kind = match &part {
                Part::Instruction(_) => "instruction",
                Part::Trusted(_) => "trusted",
                Part::Untrusted(_) => "untrusted",
                Part::AssistantText(_) => "assistant text",
                Part::ToolCall(_) => "tool call",
                Part::ToolResult { .. } => "tool result",
                Part::EngineResult { .. } => "engine result",
            };
            found.push((canary, kind.to_string()));
        }
    }
    found
}

/// No canary anywhere in `request`, in its parts or its debug form.
fn assert_no_canary(who: &str, request: &ChatRequest) {
    assert!(
        canaries_in(request).is_empty(),
        "{who}: {:?}",
        canaries_in(request)
    );
    let all = format!("{request:?}");
    for canary in CANARIES {
        assert!(!all.contains(canary), "{who}: `{canary}` in {all}");
    }
}

/// Each engine result in `request` is ToolGate's for a committed row:
/// built from the row id, the version and the status word, and nothing
/// else (no canary).
fn assert_engine_values_only(who: &str, request: &ChatRequest, rows: &[am_store::RowRecord]) {
    for (_, result) in engine_results(request) {
        assert_eq!(result.label(), WRITE_LABEL, "{who}");
        let text = result.text();
        for canary in CANARIES {
            assert!(!text.contains(canary), "{who}: `{canary}` in {text}");
        }
        let built_from_engine_values = rows.iter().any(|row| {
            let row_id = uuid::Uuid::parse_str(&row.row_id).unwrap();
            (1..=row.version).any(|version| {
                // Review round 2, T2: `AlreadyWritten` is removed; a write
                // is an insert or an update.
                [RowStatus::Inserted, RowStatus::Updated]
                    .into_iter()
                    .any(|status| {
                        EngineResult::row_written(&RowWritten {
                            row_id,
                            version,
                            status,
                        })
                        .text()
                            == text
                    })
            })
        });
        assert!(
            built_from_engine_values,
            "{who}: built from a row id, a version and a word: {text}"
        );
    }
}

/// Row 43 (owner condition b), with a clean writer and a tainted one. The
/// run succeeds and the canaries are in the rows (they were written). A's
/// first request holds none; A's later requests hold each canary only in
/// A's own replayed `Part::ToolCall` arguments, in assistant messages. B's
/// and C's requests hold none anywhere. Every engine result holds only
/// engine values.
#[tokio::test]
async fn no_row_content_reaches_a_later_model_request() {
    for tainted_writer in [false, true] {
        let ran = three_steps(tainted_writer).await;
        let who = if tainted_writer {
            "tainted A"
        } else {
            "clean A"
        };

        assert_eq!(ran.outcome.status, "succeeded", "{who}: {:?}", ran.outcome);
        let rows = rows_of(&ran.p.db, TABLE);
        assert_eq!(rows.len(), 2, "{who}: {rows:#?}");
        let stored: String = rows.iter().map(|r| r.data.clone()).collect();
        for canary in CANARIES {
            assert!(stored.contains(canary), "{who}: `{canary}` was written");
        }
        let revised = rows
            .iter()
            .find(|r| data_of(&r.data)["state"] == "done")
            .expect("B revised a canary row");
        assert_eq!(revised.version, 2, "{who}");
        assert!(data_of(&revised.data)["title"] == TITLE_CANARY, "{who}");

        let a = ran.a.requests();
        assert_eq!(a.len(), 3, "{who}: A's three turns");
        assert_no_canary(&format!("{who}: A's first request"), &a[0]);
        for (n, request) in a.iter().enumerate().skip(1) {
            for (canary, kind) in canaries_in(request) {
                assert_eq!(
                    kind, "tool call",
                    "{who}: A's request {n} holds `{canary}` only in its own replayed call"
                );
            }
            for message in &request.messages {
                for part in &message.parts {
                    let text = part_text(part);
                    if CANARIES.iter().any(|c| text.contains(c)) {
                        assert_eq!(
                            message.role,
                            Role::Assistant,
                            "{who}: A's request {n}: {part:?}"
                        );
                        assert!(
                            matches!(part, Part::ToolCall(call) if call.name == "table_write"),
                            "{who}: A's request {n}: {part:?}"
                        );
                    }
                }
            }
            for instruction in &request.system {
                for canary in CANARIES {
                    assert!(!instruction.text().contains(canary), "{who}: system");
                }
            }
            assert_engine_values_only(&format!("{who}: A's request {n}"), request, &rows);
        }
        assert!(
            canaries_in(&a[2])
                .iter()
                .any(|(_, kind)| kind == "tool call"),
            "{who}: A's own calls are replayed to A (the control)"
        );

        let b = ran.b.requests();
        assert_eq!(b.len(), 2, "{who}: B's two turns");
        for (n, request) in b.iter().enumerate() {
            assert_no_canary(&format!("{who}: B's request {n}"), request);
            assert_engine_values_only(&format!("{who}: B's request {n}"), request, &rows);
        }
        assert_eq!(
            engine_results(&b[1]).len(),
            1,
            "{who}: B read its own write's engine result"
        );

        let c = ran.c.requests();
        assert_eq!(c.len(), 1, "{who}: C's one turn");
        assert_no_canary(&format!("{who}: C's request"), &c[0]);
        assert!(c[0].tools.is_empty(), "{who}: C has no tools");
    }
}

/// Row 43 (owner condition a, at run time; since 3.6c, for steps that fill
/// no `row_inputs`): no such step's journaled input names a row. Every
/// input of every attempt of B and C is one of the kinds of stories 3.3
/// and 3.4, from a step run of this run or a plan input, names no
/// after-image (`from_row`), and none is labelled with a row's or a
/// table's id. B, which read only a clean producer's output and its own
/// engine result, stays clean.
#[tokio::test]
async fn no_step_input_names_a_row() {
    for tainted_writer in [false, true] {
        let ran = three_steps(tainted_writer).await;
        let who = if tainted_writer {
            "tainted A"
        } else {
            "clean A"
        };
        assert_eq!(ran.outcome.status, "succeeded", "{who}: {:?}", ran.outcome);
        let j = journal(&ran.p.db);
        let rows = rows_of(&ran.p.db, TABLE);
        let attempts: Vec<String> = j.steps.iter().map(|s| s.id.clone()).collect();

        for key in ["revise", "close"] {
            let attempt = j.attempt(key, 1);
            for input in j.inputs_of(&attempt.id) {
                assert!(
                    INPUT_KINDS.contains(&input.kind.as_str()),
                    "{who}: {key}: {input:?}"
                );
                assert_eq!(input.from_row, None, "{who}: {key}: {input:?}");
                if let Some(from) = &input.from_step_run {
                    assert!(attempts.contains(from), "{who}: {key}: {input:?}");
                }
                assert!(!input.label.contains(TABLE), "{who}: {key}: {input:?}");
                for row in &rows {
                    assert!(
                        !input.label.contains(&row.row_id),
                        "{who}: {key}: {input:?}"
                    );
                }
            }
        }
        let revise = j.attempt("revise", 1);
        let expected = if tainted_writer {
            // A tainted producer's output reaches B fenced (AD-10): B is
            // tainted by A's words, never by a row.
            Taint::Tainted
        } else {
            Taint::Clean
        };
        assert_eq!(revise.taint, expected, "{who}: {revise:?}");
    }
}

/// Row 43 widened by story 3.6c (rows 38 and 47; owner Q4's reader
/// condition): the slot is the one read path. After A, B and C, a later
/// run's step `read` names both rows in its `row_inputs`. Each canary
/// reaches `read`'s request in exactly one part: the fence of its own row,
/// labelled `row:entries/<id>@<version>`; no system, instruction, trusted,
/// tool-result or engine-result part holds one. `read` is tainted, with one
/// `row` edge per row, in the order named, naming the row's current
/// after-image and carrying at least its taint; A's, B's and C's inputs
/// still name no row.
#[tokio::test]
async fn the_slot_is_the_one_read_path_and_it_fences_and_taints() {
    for tainted_writer in [false, true] {
        let ran = three_steps(tainted_writer).await;
        let who = if tainted_writer {
            "tainted A"
        } else {
            "clean A"
        };
        assert_eq!(ran.outcome.status, "succeeded", "{who}: {:?}", ran.outcome);
        let rows = rows_of(&ran.p.db, TABLE);
        let ids: Vec<&str> = rows.iter().map(|r| r.row_id.as_str()).collect();
        let labels: Vec<String> = rows
            .iter()
            .map(|r| format!("row:{TABLE}/{}@{}", r.row_id, r.version))
            .collect();
        let d = texts(&["read both"]);

        let outcome = run_plan(
            &ran.p.store,
            owner(),
            "manual",
            &plan(vec![reading(step("read", d.clone()), &ids)]),
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert_eq!(outcome.status, "succeeded", "{who}: {outcome:?}");
        let request = d.requests()[0].clone();
        for canary in CANARIES {
            let holders = carriers(&request, canary);
            assert_eq!(holders.len(), 1, "{who}: `{canary}`: {holders:#?}");
            match &holders[0] {
                Part::Untrusted(data) => assert!(
                    labels.contains(&data.label().to_string()),
                    "{who}: `{canary}` in its row's fence: {}",
                    data.label()
                ),
                other => panic!("{who}: `{canary}` only fenced, not {other:?}"),
            }
        }
        for (canary, kind) in canaries_in(&request) {
            assert_eq!(kind, "untrusted", "{who}: `{canary}`");
        }
        for instruction in &request.system {
            for canary in CANARIES {
                assert!(!instruction.text().contains(canary), "{who}: system");
            }
        }

        let j = journal(&ran.p.db);
        let read = j.attempt("read", 1);
        assert_eq!(read.taint, Taint::Tainted, "{who}: the reader is tainted");
        let edges: Vec<_> = j
            .inputs_of(&read.id)
            .into_iter()
            .filter(|i| i.kind == "row")
            .collect();
        assert_eq!(edges.len(), rows.len(), "{who}: {edges:#?}");
        for ((row, label), edge) in rows.iter().zip(&labels).zip(&edges) {
            let image = history_of(&ran.p.db, TABLE, &row.row_id)
                .last()
                .unwrap()
                .clone();
            assert_eq!(&edge.label, label, "{who}");
            assert_eq!(edge.from_row, Some(image.seq), "{who}: {edge:?}");
            assert_eq!(edge.taint, row.taint, "{who}: the row's mark: {edge:?}");
            assert_eq!(
                (edge.from_step_run.as_deref(), edge.from_item.as_deref()),
                (None, None),
                "{who}"
            );
        }
        for key in ["collect", "revise", "close"] {
            let attempt = j.attempt(key, 1);
            for input in j.inputs_of(&attempt.id) {
                assert!(
                    input.kind != "row" && input.from_row.is_none(),
                    "{who}: {key}: {input:?}"
                );
            }
        }
    }
}
