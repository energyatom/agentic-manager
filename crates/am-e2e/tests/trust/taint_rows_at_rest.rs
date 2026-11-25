//! Trust test (AD-10 (c), restated 2026-11-24; AD-21, AD-29; epic 3 story
//! 6, matrix rows 12 and 13, through ToolGate): what a tainted step writes
//! is marked untrusted at rest, and the mark only rises.
//!
//! - A tainted step's rows carry `taint = 1` on the row and on every
//!   after-image, and each `row.written` entry says so.
//! - A row's taint never falls: a clean step that later rewrites a tainted
//!   row leaves the row tainted, and the new after-image records the row's
//!   taint after that write (1), not the writer's (AD-10 restated
//!   2026-11-24). The earlier after-images are unchanged. A direct `UPDATE`
//!   that lowers the taint is aborted by the schema.
//!
//! FakeLlm and a live model that names the row the engine minted; no
//! network, no real model. Written before the row store exists.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

use am_domain::Taint;
use am_engine::run::{RunOutcome, StepPlan, run_plan};
use rusqlite::{Connection, params};
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

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

/// The turns of a step that inserts one row, then updates it (read at
/// version `read_at`), then ends.
fn insert_then_update(read_at: i64) -> Vec<Turn> {
    vec![
        write(insert(json!({ "ref": "r-1", "title": "first" }))),
        write_with(move |r| update(&newest_row(r, TABLE), read_at, json!({ "title": "second" }))),
        say("done"),
    ]
}

/// Row 12: a step with an untrusted plan input inserts a row and updates
/// it. The row is at version 2 with taint 1; both after-images carry taint
/// 1; both `row.written` entries carry the taint.
#[tokio::test]
async fn a_tainted_steps_rows_carry_taint_in_row_and_history() {
    let p = project();
    let model = Live::new(&p.db, insert_then_update(1));

    let outcome = run(&p, vec![poisoned(writing(step("collect", model.clone())))]).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let row = only_row(&p.db, TABLE);
    assert_eq!((row.version, row.taint), (2, Taint::Tainted), "{row:?}");
    let images = history_of(&p.db, TABLE, &row.row_id);
    assert_eq!(
        images
            .iter()
            .map(|i| (i.version, i.taint))
            .collect::<Vec<_>>(),
        vec![(1, Taint::Tainted), (2, Taint::Tainted)]
    );
    let j = journal(&p.db);
    let written = j.entries("row.written");
    assert_eq!(written.len(), 2, "{written:#?}");
    for entry in &written {
        assert!(audited_tainted(&entry["args"]["taint"]), "{entry}");
    }
}

/// Row 13: run 1's tainted step writes the row to version 2 (taint 1). Run
/// 2's clean step updates it to version 3: the row stays tainted, version
/// 3's after-image records taint 1 (the row's after the write, AD-10
/// restated 2026-11-24), versions 1 and 2 are unchanged, and the clean
/// writer stays clean. A raw `UPDATE` lowering the taint is aborted and
/// changes nothing.
#[tokio::test]
async fn taint_never_falls_on_a_clean_rewrite() {
    let p = project();
    let first = Live::new(&p.db, insert_then_update(1));
    let outcome = run(&p, vec![poisoned(writing(step("collect", first)))]).await;
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let row = only_row(&p.db, TABLE);
    assert_eq!((row.version, row.taint), (2, Taint::Tainted));
    let before = history_of(&p.db, TABLE, &row.row_id);

    let second = Live::new(
        &p.db,
        vec![
            write_with(|r| {
                update(
                    &newest_row(r, TABLE),
                    newest_version(r, TABLE),
                    json!({ "title": "third, from a clean step" }),
                )
            }),
            say("done"),
        ],
    );
    let outcome = run(&p, vec![writing(step("rewrite", second))]).await;

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let after = only_row(&p.db, TABLE);
    assert_eq!(after.row_id, row.row_id);
    assert_eq!(after.version, 3);
    assert_eq!(after.taint, Taint::Tainted, "a row's taint only rises");
    assert_eq!(data_of(&after.data)["title"], "third, from a clean step");
    let images = history_of(&p.db, TABLE, &row.row_id);
    assert_eq!(images.len(), 3, "{images:#?}");
    assert_eq!(&images[..2], &before[..], "versions 1 and 2 are unchanged");
    assert_eq!(
        images[2].taint,
        Taint::Tainted,
        "version 3's after-image records the row's taint, not the writer's"
    );
    let j = journal(&p.db);
    assert_eq!(
        j.attempt("rewrite", 1).taint,
        Taint::Clean,
        "writing a tainted row does not taint the writer (its result is the engine's)"
    );

    let c = Connection::open(&p.db).unwrap();
    for sql in [
        "UPDATE table_rows SET taint = 0 WHERE row_id = ?1",
        "UPDATE table_rows SET taint = 0, version = version + 1 WHERE row_id = ?1",
    ] {
        assert!(
            c.execute(sql, params![row.row_id]).is_err(),
            "the schema aborts: {sql}"
        );
    }
    drop(c);
    assert_eq!(only_row(&p.db, TABLE), after, "nothing changed");
    assert_eq!(history_of(&p.db, TABLE, &row.row_id), images);
}
