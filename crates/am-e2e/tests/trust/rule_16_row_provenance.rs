//! Trust test (RULE-16: provenance and the audit log; AD-21, AD-24, AD-25,
//! AD-32's keyed-hash rule of 2026-11-24, AD-43; epic 3 story 6, matrix
//! rows 2, 3 and 22, and the ticket's verify): every row change has its
//! after-image, and the audit log holds ids, never content.
//!
//! - Each insert and update of a row appends one after-image (the whole
//!   row) naming the Seat actor, the run, the step attempt, the seat, the
//!   call sequence and the tool call it finished; the row's version is its
//!   newest after-image's, and each succeeded `table_write` call has its
//!   after-image (`Reader::row_written_at`).
//! - `row.written` and `document.versioned` entries hold ids, the op, the
//!   version, the call sequence, counts and the taint; a canary in row data
//!   and in a document's Markdown is in no entry, and neither is its hash.
//! - The chain verifies after row and document writes, in the store and in
//!   the export.
//! - No new hash argument of free text: the only `*_sha256` argument of a
//!   run is 3.5's `output_sha256`, each the existing helper's hash of text
//!   the journal stores.
//!
//! A live model names the rows the engine minted; no network, no real
//! model. Written before the row store exists.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

use am_domain::{Actor, Taint};
use am_engine::run::{RunOutcome, StepPlan, run_plan};
use am_store::Reader;
use am_store::audit::sha256_hex;
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

/// In a row's data (a text column and a JSON column).
const ROW_CANARY: &str = "canary-row-content-2c44";
/// In a document's Markdown.
const MD_CANARY: &str = "canary-markdown-content-e815";

/// An audit argument as a number, whether the writer stored it as a JSON
/// number or as its decimal text.
fn number(arg: &serde_json::Value) -> Option<i64> {
    arg.as_i64()
        .or_else(|| arg.as_str().and_then(|s| s.parse().ok()))
}

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

/// The Markdown note `summarise` writes, holding [`MD_CANARY`].
fn markdown() -> String {
    format!("# Note\n\nThe secret line: {MD_CANARY}.\n")
}

/// `collect` inserts two rows and updates the first (canaries in its data),
/// then `summarise` writes a note.
async fn rows_and_a_note(p: &Project) -> RunOutcome {
    rows_then(
        p,
        step("summarise", texts(&["Three changes were recorded."])),
    )
    .await
}

/// As [`rows_and_a_note`], but `summarise` declares [`DOCUMENT`] and writes
/// [`markdown`] (story 3.6b, the document half of row 22).
async fn rows_and_a_document(p: &Project) -> RunOutcome {
    let markdown = markdown();
    rows_then(
        p,
        documenting(step("summarise", texts(&[markdown.as_str()])), DOCUMENT),
    )
    .await
}

/// `collect` inserts two rows and updates the first (canaries in its
/// data), then `last` runs.
async fn rows_then(p: &Project, last: StepPlan) -> RunOutcome {
    let collect = Live::new(
        &p.db,
        vec![
            write(insert(json!({
                "ref": "r-1",
                "title": ROW_CANARY,
                "extra": { "note": ROW_CANARY },
            }))),
            write(insert(json!({ "ref": "r-2", "amount": 2.5 }))),
            write_with(|r| {
                let first = r
                    .table_rows_of(TABLE)
                    .unwrap()
                    .into_iter()
                    .find(|row| data_of(&row.data)["ref"] == "r-1")
                    .unwrap();
                update(&first.row_id, 1, json!({ "count": 3, "state": "done" }))
            }),
            say("recorded"),
        ],
    );
    let outcome = run(p, vec![writing(step("collect", collect)), last]).await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    outcome
}

/// Rows 2 and 3 (the verify): every row change has an after-image naming
/// the run, the step attempt and the seat. Two inserts and one update give
/// three after-images; each names `Actor::Seat` of this run and step, the
/// run, the attempt, the seat, the call sequence and the id of the tool
/// call it finished, with the op; the update's after-image is the whole
/// row; each row's version and data are its newest after-image's; and
/// every succeeded `table_write` call reads back its after-image.
#[tokio::test]
async fn every_row_change_has_an_after_image_naming_run_step_and_seat() {
    let p = project();
    let outcome = rows_and_a_note(&p).await;
    let run_uuid = uuid::Uuid::parse_str(&outcome.run_id).unwrap();
    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);
    let calls = j.tool_calls_of(&attempt.id);
    assert_eq!(calls.len(), 3, "{calls:#?}");
    let reader = Reader::open(&p.db).unwrap();

    let mut images = Vec::new();
    for row in rows_of(&p.db, TABLE) {
        let history = history_of(&p.db, TABLE, &row.row_id);
        assert_eq!(history.len() as i64, row.version, "{history:#?}");
        let newest = history.last().unwrap();
        assert_eq!(newest.version, row.version);
        assert_eq!(data_of(&newest.data), data_of(&row.data), "the whole row");
        assert_eq!(newest.taint, row.taint);
        images.extend(history);
    }
    assert_eq!(images.len(), 3);
    for image in &images {
        assert_eq!(
            image.actor,
            Actor::Seat {
                seat: "seat-collect".into(),
                run: run_uuid,
                step: "collect".into(),
            }
        );
        assert_eq!(image.run_id.as_deref(), Some(outcome.run_id.as_str()));
        assert_eq!(image.step_run_id.as_deref(), Some(attempt.id.as_str()));
        assert_eq!(image.seat.as_deref(), Some("seat-collect"));
        let seq = image.call_seq.expect("the call sequence");
        let call = calls
            .iter()
            .find(|c| c.call_seq == seq)
            .unwrap_or_else(|| panic!("no tool call {seq}: {calls:#?}"));
        assert_eq!(image.tool_call_id.as_deref(), Some(call.id.as_str()));
        assert_eq!(image.taint, Taint::Clean);
        let op = if image.version == 1 {
            "insert"
        } else {
            "update"
        };
        assert_eq!(image.op, op, "{image:?}");
    }
    let updated = images.iter().find(|i| i.op == "update").unwrap();
    let whole = data_of(&updated.data);
    assert_eq!(
        (
            &whole["ref"],
            &whole["title"],
            &whole["count"],
            &whole["state"]
        ),
        (&json!("r-1"), &json!(ROW_CANARY), &json!(3), &json!("done")),
        "an after-image is the whole row, not the patch"
    );
    for call in &calls {
        assert_eq!(call.status, "succeeded");
        let image = reader
            .row_written_at(&attempt.id, call.call_seq)
            .unwrap()
            .expect("a succeeded call's after-image");
        assert_eq!(image.tool_call_id.as_deref(), Some(call.id.as_str()));
    }
}

/// Row 22: the audit log holds ids, never content. `row.written` targets
/// `row:entries/<row id>` with the op, the version, the attempt, the call
/// sequence and the taint; `document.versioned` names the document with
/// its version, byte count and taint. Neither canary, nor its hash, nor
/// the hash of a row's data is in the export.
#[tokio::test]
async fn audit_holds_ids_never_row_content() {
    let p = project();
    rows_and_a_document(&p).await;
    let j = journal(&p.db);
    let attempt = j.attempt("collect", 1);

    let written = j.entries("row.written");
    assert_eq!(written.len(), 3, "{written:#?}");
    let rows = rows_of(&p.db, TABLE);
    for (n, entry) in written.iter().enumerate() {
        let target = entry["target"].as_str().unwrap_or_default();
        assert!(
            rows.iter()
                .any(|r| target == format!("row:{TABLE}/{}", r.row_id)),
            "{entry}"
        );
        let args = &entry["args"];
        assert!(args["op"] == "insert" || args["op"] == "update", "{entry}");
        assert!(matches!(number(&args["version"]), Some(1 | 2)), "{entry}");
        assert_eq!(number(&args["call_seq"]), Some(n as i64 + 1), "{entry}");
        assert_eq!(args["step_run_id"], json!(attempt.id), "{entry}");
        assert!(audited_clean(&args["taint"]), "{entry}");
    }
    let versioned = j.entries("document.versioned");
    assert_eq!(versioned.len(), 1, "{versioned:#?}");
    let entry = &versioned[0];
    assert!(entry.to_string().contains(DOCUMENT), "{entry}");
    assert_eq!(number(&entry["args"]["version"]), Some(1), "{entry}");
    assert!(audited_clean(&entry["args"]["taint"]), "{entry}");
    assert_eq!(
        number(&entry["args"]["bytes"]),
        Some(markdown().len() as i64),
        "{entry}"
    );

    let export = serde_json::to_string(&j.audit).unwrap();
    for canary in [ROW_CANARY, MD_CANARY] {
        assert!(!export.contains(canary), "`{canary}` in the audit log");
        assert!(
            !export.contains(&sha256_hex(canary)),
            "the hash of `{canary}` in the audit log"
        );
    }
    for row in &rows {
        assert!(
            !export.contains(&sha256_hex(&row.data)),
            "a row's data is never hashed into the log"
        );
        for image in history_of(&p.db, TABLE, &row.row_id) {
            assert!(!export.contains(&sha256_hex(&image.data)));
        }
    }
}

/// Row 22: after row and document writes the chain verifies in the store,
/// and the export verifies on its own.
#[tokio::test]
async fn the_chain_verifies_after_row_and_document_writes() {
    let p = project();
    rows_and_a_document(&p).await;
    let reader = Reader::open(&p.db).unwrap();

    assert_eq!(reader.verify_audit_chain().unwrap(), None);
    let mut out = Vec::new();
    let (count, head) = reader.export_audit_jsonl(&mut out).unwrap();
    let ok = am_store::verify_jsonl(out.as_slice()).unwrap();
    assert_eq!((ok.entries, ok.head), (count, head));
    let text = String::from_utf8(out).unwrap();
    for action in [
        "\"row.written\"",
        "\"document.versioned\"",
        "\"inbox.created\"",
    ] {
        assert!(text.contains(action), "{action} is in the chain");
    }
}

/// Row 22 (AD-32's keyed-hash rule, 2026-11-24): across a run that writes
/// rows and a document, the only hash argument is the existing
/// `output_sha256`, each value the existing helper's hash of text the
/// journal stores (a step's output); no new `*_sha256` key appears.
#[tokio::test]
async fn no_new_sha256_arg_of_free_text() {
    let p = project();
    rows_and_a_document(&p).await;
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
                stored.iter().any(|t| sha256_hex(t) == hash),
                "{entry}: not the helper's hash of stored text"
            );
            hashes += 1;
        }
    }
    assert!(hashes >= 2, "both steps' finishes hash their outputs");
}
