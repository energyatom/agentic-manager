//! Engine test (AD-8, AD-28, AD-29, AD-35, AD-43; epic 3 story 6, plan
//! matrix rows 23 to 25, 27 and 28, and the ticket's verify "a document
//! step leaves one Markdown version and Draft items in the Inbox"): a step
//! that declares `document: <id>` stores its final message as a version of
//! that document when it succeeds, and raises one Draft item for it, in the
//! transaction that finishes the step.
//!
//! - One version per succeeded attempt (the owner's approved deviation
//!   from "one version per run", Q7): numbered per document across runs; a
//!   second attempt in the same run adds one; none is ever replaced.
//! - A version names its run, attempt and step run, carries the attempt's
//!   taint and its size; the Markdown stays once, on the attempt's output.
//! - The Draft item's payload is the producing attempt, its words the
//!   engine's; it never blocks the run.
//! - An empty, whitespace-only or NUL-holding final message, or one over
//!   1 MiB, fails the step with a Failure item and no version.
//! - A plan that pairs a document with an output schema, a loop-back or a
//!   gate, or puts a table or a document on a gate, is refused before any
//!   row.
//!
//! No network, no real model. Written before any of it exists.
#![forbid(unsafe_code)]

mod support;

use std::sync::Arc;

use am_domain::{InboxKind, LlmProvider, Taint};
use am_engine::run::{RunOutcome, RunPlan, StepPlan, run_plan};
use am_store::Reader;
use am_testkit::FakeLlm;
use serde_json::Value;
use support::*;
use tokio_util::sync::CancellationToken;

const MARKDOWN: &str = "# Summary\n\n- one entry recorded\n- nothing left open\n";

async fn run(p: &Project, plan: &RunPlan) -> RunOutcome {
    run_plan(&p.store, owner(), "manual", plan, CancellationToken::new())
        .await
        .unwrap()
}

/// A document step `summarise` on `provider`.
fn summarise(provider: Arc<dyn LlmProvider>) -> StepPlan {
    documenting(step("summarise", provider), DOCUMENT)
}

fn number(arg: &Value) -> Option<i64> {
    arg.as_i64()
        .or_else(|| arg.as_str().and_then(|s| s.parse().ok()))
}

/// Every version of `document`, in version order (3.6b's reader; kept out
/// of `support/mod.rs`, which every engine and trust target includes, so
/// those targets compile before `Reader::document_versions` exists).
fn versions_of(db: &std::path::Path, document: &str) -> Vec<am_store::DocumentVersionRecord> {
    Reader::open(db)
        .unwrap()
        .document_versions(document)
        .unwrap()
}

/// Row 23, the verify: the step succeeds with the Markdown as its output;
/// one version of `summary-note` (version 1, the run, attempt 1, the step
/// run, clean, its size in bytes); one open Draft item on that attempt
/// (payload the attempt, clean, title and body naming the document and the
/// version and holding none of the Markdown); the run succeeds (a Draft
/// never blocks it); `document.versioned` and `inbox.created` audited, the
/// Markdown in no audit entry.
#[tokio::test]
async fn a_document_step_leaves_one_version_and_one_draft() {
    let p = project();

    let outcome = run(&p, &plan(vec![summarise(texts(&[MARKDOWN]))])).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let attempt = j.attempt("summarise", 1);
    assert_eq!(attempt.status, "succeeded");
    assert_eq!(attempt.output.as_deref(), Some(MARKDOWN));
    let versions = versions_of(&p.db, DOCUMENT);
    assert_eq!(versions.len(), 1, "{versions:#?}");
    let v = &versions[0];
    assert_eq!(v.document_id, DOCUMENT);
    assert_eq!(v.version, 1);
    assert_eq!(v.run_id, outcome.run_id);
    assert_eq!(v.attempt, 1);
    assert_eq!(v.step_run_id, attempt.id);
    assert_eq!(v.taint, Taint::Clean);
    assert_eq!(v.bytes, MARKDOWN.len() as i64);

    assert_eq!(j.items.len(), 1, "{:#?}", j.items);
    let draft = &j.items[0];
    assert_eq!(draft.kind, InboxKind::Draft);
    assert_eq!(draft.status, "open");
    assert_eq!(draft.step_run_id, attempt.id);
    assert_eq!(draft.payload.as_deref(), Some(attempt.id.as_str()));
    assert_eq!(draft.taint, Taint::Clean);
    let words = format!("{}\n{}", draft.title, draft.body);
    assert!(words.contains(DOCUMENT), "{words}");
    assert!(holds_version(&words, 1), "{words}");
    assert!(
        !words.contains("Summary") && !words.contains("entry recorded"),
        "engine words only: {words}"
    );

    let versioned = j.entries("document.versioned");
    assert_eq!(versioned.len(), 1, "{versioned:#?}");
    assert!(
        versioned[0].to_string().contains(DOCUMENT),
        "{}",
        versioned[0]
    );
    assert_eq!(number(&versioned[0]["args"]["version"]), Some(1));
    assert_eq!(
        number(&versioned[0]["args"]["bytes"]),
        Some(MARKDOWN.len() as i64)
    );
    assert!(
        audited_clean(&versioned[0]["args"]["taint"]),
        "{}",
        versioned[0]
    );
    assert_eq!(j.entries("inbox.created").len(), 1);
    let export = serde_json::to_string(&j.audit).unwrap();
    assert!(!export.contains("entry recorded"), "{export}");
}

/// Row 24: a second run of the same workflow adds version 2 of the
/// document; version 1 is unchanged. Review round 2, fix 3 (tester,
/// 2026-11-25): each Draft's title and body name its own version's number,
/// the one the journal committed, and not the other's.
#[tokio::test]
async fn a_second_run_adds_version_two() {
    let p = project();
    let model = texts(&["# First version", "# Second version"]);
    let plan = plan(vec![summarise(model)]);

    let first = run(&p, &plan).await;
    let one = versions_of(&p.db, DOCUMENT);
    let second = run(&p, &plan).await;

    assert_eq!(
        (first.status.as_str(), second.status.as_str()),
        ("succeeded", "succeeded")
    );
    let both = versions_of(&p.db, DOCUMENT);
    assert_eq!(both.len(), 2, "{both:#?}");
    assert_eq!(both[0], one[0], "version 1 is unchanged");
    assert_eq!((both[0].version, both[1].version), (1, 2));
    assert_eq!(both[0].run_id, first.run_id);
    assert_eq!(both[1].run_id, second.run_id);
    let drafts: Vec<_> = journal(&p.db)
        .items
        .into_iter()
        .filter(|i| i.kind == InboxKind::Draft)
        .collect();
    assert_eq!(drafts.len(), 2, "one Draft per version");
    for (v, other) in both.iter().zip([2, 1]) {
        let draft = drafts
            .iter()
            .find(|d| d.payload.as_deref() == Some(v.step_run_id.as_str()))
            .unwrap_or_else(|| panic!("the Draft of version {}", v.version));
        for words in [&draft.title, &draft.body] {
            assert!(
                holds_version(words, v.version) && !holds_version(words, other),
                "the Draft of version {} names it: {words}",
                v.version
            );
        }
    }
}

/// Row 25 (owner Q7, the approved deviation from "one version per run"):
/// `check` loops back to the document step once, so its attempt 2 runs in
/// the same run: version 2 with its own Draft item; version 1 and its item
/// unchanged; both versions name the run, each its own attempt.
#[tokio::test]
async fn a_returned_document_step_adds_a_version_in_the_same_run() {
    let p = project();
    let check = texts(&[r#"{"reasons":["shorter"]}"#, r#"{"reasons":[]}"#]);
    let plan = plan(vec![
        summarise(texts(&["# Long note", "# Short note"])),
        looping("check", check, "summarise", 1),
    ]);

    let outcome = run(&p, &plan).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&p.db);
    let (one, two) = (j.attempt("summarise", 1), j.attempt("summarise", 2));
    let versions = versions_of(&p.db, DOCUMENT);
    assert_eq!(versions.len(), 2, "{versions:#?}");
    assert_eq!(
        versions
            .iter()
            .map(|v| (
                v.version,
                v.attempt,
                v.step_run_id.clone(),
                v.run_id.clone()
            ))
            .collect::<Vec<_>>(),
        vec![
            (1, 1, one.id.clone(), outcome.run_id.clone()),
            (2, 2, two.id.clone(), outcome.run_id.clone()),
        ]
    );
    let drafts: Vec<_> = j
        .items
        .iter()
        .filter(|i| i.kind == InboxKind::Draft)
        .collect();
    assert_eq!(drafts.len(), 2, "{drafts:#?}");
    assert_eq!(drafts[0].payload.as_deref(), Some(one.id.as_str()));
    assert_eq!(drafts[1].payload.as_deref(), Some(two.id.as_str()));
    assert!(drafts.iter().all(|d| d.status == "open"));
    assert_eq!(one.output.as_deref(), Some("# Long note"));
}

/// Row 27: a final message that is empty, only whitespace, holds a NUL or
/// is over 1 MiB fails the document step with one Failure item; no version
/// and no Draft item is written.
#[tokio::test]
async fn an_empty_document_fails_with_a_failure_item() {
    let too_long = "x".repeat(1024 * 1024 + 1);
    for (what, text) in [
        ("empty", ""),
        ("whitespace", "  \n\t \n"),
        ("a NUL", "# Note\u{0}hidden"),
        ("over 1 MiB", too_long.as_str()),
    ] {
        let p = project();

        let outcome = run(&p, &plan(vec![summarise(texts(&[text]))])).await;

        assert_eq!(outcome.status, "failed", "{what}: {outcome:?}");
        assert!(versions_of(&p.db, DOCUMENT).is_empty(), "{what}");
        let j = journal(&p.db);
        let attempt = j.attempt("summarise", 1);
        assert_eq!(attempt.status, "failed", "{what}");
        let kinds: Vec<InboxKind> = j.items.iter().map(|i| i.kind).collect();
        assert_eq!(kinds, [InboxKind::Failure], "{what}: {:#?}", j.items);
        assert_eq!(j.items[0].step_run_id, attempt.id, "{what}");
        assert!(j.entries("document.versioned").is_empty(), "{what}");
    }
}

/// Row 27, review round 2, fix 5 (C3; tester, 2026-11-25): the 1 MiB limit
/// counts bytes, not characters. A final message of multi-byte characters
/// that is under 1 Mi characters but over 1 MiB in bytes (2-byte `é`,
/// 4-byte emoji) fails the step with one Failure item and no version. The
/// control: exactly 1 MiB in bytes (`é` 524,288 times) is a version of
/// 1,048,576 bytes.
#[tokio::test]
async fn a_document_is_measured_in_bytes() {
    const MIB: usize = 1024 * 1024;
    let two_byte = "\u{e9}".repeat(600_000);
    let four_byte = "\u{1F600}".repeat(262_145);
    for (what, text) in [("2-byte", &two_byte), ("4-byte", &four_byte)] {
        assert!(text.chars().count() < MIB && text.len() > MIB, "{what}");
        let p = project();

        let outcome = run(&p, &plan(vec![summarise(texts(&[text.as_str()]))])).await;

        assert_eq!(outcome.status, "failed", "{what}: {outcome:?}");
        assert!(versions_of(&p.db, DOCUMENT).is_empty(), "{what}");
        let j = journal(&p.db);
        let kinds: Vec<InboxKind> = j.items.iter().map(|i| i.kind).collect();
        assert_eq!(kinds, [InboxKind::Failure], "{what}: {:#?}", j.items);
    }

    let exactly = "\u{e9}".repeat(MIB / 2);
    assert_eq!(exactly.len(), MIB);
    let p = project();
    let outcome = run(&p, &plan(vec![summarise(texts(&[exactly.as_str()]))])).await;
    assert_eq!(outcome.status, "succeeded", "the control: {outcome:?}");
    let versions = versions_of(&p.db, DOCUMENT);
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].bytes, MIB as i64);
}

/// Row 28: a hand-built plan that pairs a document with an output schema,
/// with a loop-back or with a gate, or declares a table or a document on a
/// gate step, is refused by `run_plan` with an error naming the step,
/// before any row: no run, and no model called. The control (tester,
/// 2026-11-25): the same document step after the same `first` step, with
/// none of those, is not refused, so the refusal is the mix's, not the
/// document's (3.6a refused every document). Review round 2, fix 7 (S6;
/// tester, 2026-11-25): each refusal names its own clash (`schema`,
/// `loop`, `gate`), so the loop-back case passes only through the
/// engine's loop-back check, and a second loop-back case has the document
/// step loop back past a step between.
#[tokio::test]
async fn a_mixed_declaration_is_refused_before_any_row() {
    for case in 0..5 {
        let p = project();
        let model = never();
        let m: Arc<dyn LlmProvider> = model.clone();
        let (what, clash, steps) = match case {
            0 => (
                "a document with an output schema",
                "schema",
                vec![with_schema(summarise(m), line_schema())],
            ),
            1 => (
                "a document with a loop-back",
                "loop",
                vec![
                    step("first", m.clone()),
                    documenting(looping("summarise", m, "first", 1), DOCUMENT),
                ],
            ),
            2 => (
                "a document with a loop-back past a step between",
                "loop",
                vec![
                    step("first", m.clone()),
                    step("middle", m.clone()),
                    documenting(looping("summarise", m, "first", 2), DOCUMENT),
                ],
            ),
            3 => (
                "a document on a gate",
                "gate",
                vec![
                    step("first", m.clone()),
                    documenting(gate("summarise", m), DOCUMENT),
                ],
            ),
            _ => (
                "a table on a gate",
                "gate",
                vec![step("first", m.clone()), writing(gate("summarise", m))],
            ),
        };

        let refused = run_plan(
            &p.store,
            owner(),
            "manual",
            &plan(steps),
            CancellationToken::new(),
        )
        .await;

        let words = match refused {
            Err(e) => e.to_string(),
            Ok(outcome) => panic!("{what}: refused before any row: {outcome:?}"),
        };
        assert!(
            words.contains("summarise"),
            "{what}: names the step: {words}"
        );
        assert!(words.contains(clash), "{what}: names `{clash}`: {words}");
        assert!(
            Reader::open(&p.db).unwrap().runs().unwrap().is_empty(),
            "{what}: no run row"
        );
        assert_eq!(model.calls(), 0, "{what}: no model called");
    }

    let p = project();
    let outcome = run_plan(
        &p.store,
        owner(),
        "manual",
        &plan(vec![
            step("first", texts(&["first done"])),
            summarise(texts(&[MARKDOWN])),
        ]),
        CancellationToken::new(),
    )
    .await
    .unwrap_or_else(|e| panic!("the control: a document step alone is not refused: {e}"));
    assert_eq!(outcome.status, "succeeded", "the control: {outcome:?}");
}

// --------------------------------------- review round 2 (tester, 2026-11-25)
//
// fix 6 (S4, C4; AD-21, AD-29): a hand-built plan gets the checks a config
// gets. Each refusal is `run_plan`'s, before any run row and before any
// model call; the shared id rule's function is the implementer's to name
// (none exists yet), so these tests go through `run_plan` alone (the
// store's own refusal of a bad id is in am-store's `tests/documents.rs`).

/// `run_plan` on `steps` is refused before any row: its words, after
/// checking that no run row was written and `model` was never called.
async fn refused_before_any_row(what: &str, steps: Vec<StepPlan>, model: &FakeLlm) -> String {
    let p = project();
    let refused = run_plan(
        &p.store,
        owner(),
        "manual",
        &plan(steps),
        CancellationToken::new(),
    )
    .await;
    let words = match refused {
        Err(e) => e.to_string(),
        Ok(outcome) => panic!("{what}: refused before any row: {outcome:?}"),
    };
    assert!(
        Reader::open(&p.db).unwrap().runs().unwrap().is_empty(),
        "{what}: no run row ({words})"
    );
    assert_eq!(model.calls(), 0, "{what}: no model called");
    words
}

/// fix 6: a document id is a config id (kebab-case). A plan whose document
/// step names a document with a space, a `/`, a right-to-left override
/// (U+202E) or a zero-width space (U+200B) is refused, naming the step.
#[tokio::test]
async fn a_document_id_outside_the_id_rule_is_refused_before_any_row() {
    for bad in [
        "summary note",
        "summary/note",
        "summary\u{202E}note",
        "summary\u{200B}note",
    ] {
        let model = never();
        let words = refused_before_any_row(
            &format!("{bad:?}"),
            vec![documenting(step("summarise", model.clone()), bad)],
            &model,
        )
        .await;
        assert!(
            words.contains("summarise"),
            "{bad:?}: names the step: {words}"
        );
    }
}

/// fix 6: a step that versions a document or writes a table names the seat
/// that writes it. A document step and a table step whose seat is blank
/// (empty, or only spaces) are refused, naming the step.
#[tokio::test]
async fn a_writing_step_with_a_blank_seat_is_refused_before_any_row() {
    for (what, blank) in [("empty", ""), ("spaces", "  ")] {
        for (kind, plan_step) in [
            (
                "a document step",
                documenting(step("summarise", never()), DOCUMENT),
            ),
            ("a table step", writing(step("summarise", never()))),
        ] {
            let model = never();
            let blank_seat = StepPlan {
                seat: blank.into(),
                provider: model.clone(),
                ..plan_step
            };
            let words =
                refused_before_any_row(&format!("{kind}, {what} seat"), vec![blank_seat], &model)
                    .await;
            assert!(
                words.contains("summarise"),
                "{kind}, {what} seat: names the step: {words}"
            );
        }
    }
}

/// fix 6 (owner Q3: one team per document id): one document declared by two
/// steps whose seats are in different teams is refused, naming the
/// document.
#[tokio::test]
async fn one_document_of_two_teams_is_refused_before_any_row() {
    let model = never();
    let core = documenting(step("summarise", model.clone()), DOCUMENT);
    let side = StepPlan {
        seat_team: OTHER_TEAM.into(),
        ..documenting(step("note", model.clone()), DOCUMENT)
    };
    assert_ne!(core.seat_team, side.seat_team);

    let words = refused_before_any_row("two teams", vec![core, side], &model).await;

    assert!(words.contains(DOCUMENT), "names the document: {words}");
}
