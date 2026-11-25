//! Trust test (AD-10 (c) restated 2026-11-24, AD-43; epic 3 story 6, owner
//! condition Q4, matrix row 38): a row written by a tainted step taints any
//! later step that reads it, and so does every row read, because row text
//! is always `UntrustedData` (free text in rows).
//!
//! Run 1's step writes a row `r` (tainted when the step had an untrusted
//! plan input, clean otherwise). Run 2's step reads `r` through its
//! `row_inputs` slot (the slot 3.15 and 7.2 fill from config; tests fill it
//! here). At the reading attempt's start the engine journals a `row` input
//! edge, labelled `row:entries/<r>@<version>`, naming `r`'s current
//! after-image by `from_row` (its `row_history.seq`) with taint at least
//! `r`'s at-rest taint, so lineage tells the two apart; the row's data
//! enters the prompt fenced, never trusted; the reading attempt is tainted
//! either way, and the step after it gets that output fenced. Reading
//! never changes `r`: its mark and its history stay as they were.
//!
//! FakeLlm only; no network, no real model. Written before the slot.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

use std::sync::Arc;

use am_domain::Taint;
use am_engine::run::{RunOutcome, StepPlan, run_plan};
use am_store::{RowHistoryRecord, RowRecord};
use am_testkit::{FakeLlm, ScriptedTurn};
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

/// A value in the row's data the reader's prompt must carry, fenced.
const ROW_CANARY: &str = "canary-row-text-0b6e";

async fn run(p: &Project, steps: Vec<StepPlan>) -> RunOutcome {
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

/// Run 1: a step (poisoned when `tainted`) inserts one row holding
/// [`ROW_CANARY`]; the row as committed, and its after-images.
async fn written(p: &Project, tainted: bool) -> (RowRecord, Vec<RowHistoryRecord>) {
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("recording").tool_call(
            "table_write",
            &insert(json!({ "ref": "r-1", "title": ROW_CANARY })).to_string(),
        ),
        ScriptedTurn::text("recorded"),
    ]));
    let collect = writing(step("collect", llm));
    let collect = if tainted { poisoned(collect) } else { collect };
    let outcome = run(p, vec![collect]).await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let row = only_row(&p.db, TABLE);
    assert_eq!(row.taint, Taint::of_flag(tainted), "{row:?}");
    let images = history_of(&p.db, TABLE, &row.row_id);
    assert_eq!(images.len(), 1);
    (row, images)
}

/// What run 2 left: the reading attempt's journal and both models.
struct Read {
    reader: Arc<FakeLlm>,
    after: Arc<FakeLlm>,
}

/// Run 2: `read` reads `row`, then `after` runs on its output.
async fn read(p: &Project, row: &RowRecord) -> Read {
    let reader = texts(&["read it"]);
    let after = texts(&["after it"]);
    let outcome = run(
        p,
        vec![
            reading(step("read", reader.clone()), &[row.row_id.as_str()]),
            step("after", after.clone()),
        ],
    )
    .await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    Read { reader, after }
}

/// The checks every read shares: one `row` input edge naming the row's
/// current after-image, with `edge` as its taint; the row's data fenced
/// under that label, never a trusted part; the row and its history as
/// they were. Returns the edge's label.
fn assert_the_edge(
    p: &Project,
    row: &RowRecord,
    images: &[RowHistoryRecord],
    seen: &Read,
    edge: Taint,
) -> String {
    let label = format!("row:{TABLE}/{}@{}", row.row_id, row.version);
    let j = journal(&p.db);
    let attempt = j.attempt("read", 1);
    let inputs: Vec<_> = j
        .inputs_of(&attempt.id)
        .into_iter()
        .filter(|i| i.kind == "row")
        .collect();
    assert_eq!(inputs.len(), 1, "{:#?}", j.inputs_of(&attempt.id));
    let input = &inputs[0];
    assert_eq!(input.label, label);
    assert_eq!(
        input.from_row,
        Some(images.last().unwrap().seq),
        "the edge names the after-image it read (AD-43)"
    );
    assert_eq!(
        (input.from_step_run.as_deref(), input.from_item.as_deref()),
        (None, None)
    );
    assert_eq!(input.taint, edge, "the edge's taint");

    let parts = user_parts(&seen.reader.requests()[0]);
    let text = fenced(&parts, &label);
    assert!(text.contains(ROW_CANARY), "the row's data, fenced: {text}");
    assert!(no_trusted(&parts), "no row is a trusted input: {parts:#?}");
    let request = &seen.reader.requests()[0];
    for carrier in carriers(request, ROW_CANARY) {
        assert!(
            matches!(&carrier, am_domain::Part::Untrusted(d) if d.label() == label),
            "the row's text is only in its fence: {carrier:?}"
        );
    }

    assert_eq!(&only_row(&p.db, TABLE), row, "reading changes no row");
    assert_eq!(
        history_of(&p.db, TABLE, &row.row_id),
        images,
        "nor its history"
    );
    label
}

/// The reading attempt is tainted, and the step after it gets its output
/// fenced.
fn assert_the_reader_is_tainted(p: &Project, seen: &Read) {
    let j = journal(&p.db);
    assert_eq!(j.attempt("read", 1).taint, Taint::Tainted);
    assert_eq!(
        fenced(&user_parts(&seen.after.requests()[0]), "step:read@1"),
        "read it",
        "a tainted reader's output is fenced for the next step"
    );
}

/// Row 38: a row written by a tainted step taints a later reader: the edge
/// has taint 1, the reading attempt is tainted, the row's mark stays 1.
#[tokio::test]
async fn a_row_written_by_a_tainted_step_taints_a_later_reader() {
    let p = project();
    let (row, images) = written(&p, true).await;

    let seen = read(&p, &row).await;

    assert_the_edge(&p, &row, &images, &seen, Taint::Tainted);
    assert_the_reader_is_tainted(&p, &seen);
    assert_eq!(only_row(&p.db, TABLE).taint, Taint::Tainted);
}

/// Row 38: a clean row's edge is clean (lineage tells it from a tainted
/// one), and its text is still fenced `UntrustedData`.
#[tokio::test]
async fn a_clean_rows_edge_is_clean_and_its_text_still_fenced() {
    let p = project();
    let (row, images) = written(&p, false).await;

    let seen = read(&p, &row).await;

    let label = assert_the_edge(&p, &row, &images, &seen, Taint::Clean);
    let j = journal(&p.db);
    let attempt = j.attempt("read", 1);
    assert!(
        j.labels_of(&attempt.id).contains(&label),
        "the journaled prompt names the fenced row: {:?}",
        j.labels_of(&attempt.id)
    );
}

/// Row 38 (Plan Change Log, 2026-11-24): reading a clean row still taints
/// the reader: the edge's taint is 0, the attempt's is 1, because the row's
/// text entered fenced (the engine raises the attempt for every fenced row
/// input, not only from the edge's mark).
#[tokio::test]
async fn a_clean_row_read_still_taints_the_reader() {
    let p = project();
    let (row, images) = written(&p, false).await;

    let seen = read(&p, &row).await;

    assert_the_edge(&p, &row, &images, &seen, Taint::Clean);
    assert_the_reader_is_tainted(&p, &seen);
    assert_eq!(
        only_row(&p.db, TABLE).taint,
        Taint::Clean,
        "the row's own mark is not raised by a read"
    );
}
