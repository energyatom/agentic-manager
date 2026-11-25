//! Trust test (AD-10 (c), restated 2026-11-24; AD-29; story 3.6b, matrix
//! row 26): a document version and its Draft item are marked untrusted at
//! rest. A tainted document step's version and its Draft item carry taint
//! 1; a clean one's carry 0. Moved from 3.6's `taint_rows_at_rest.rs` in
//! the owner's re-slices (`taint_rows_*` names the row suite of AD-10's
//! restatement).
//!
//! FakeLlm only; no network, no real model. Written before document
//! versions exist.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

use am_domain::{InboxKind, Taint};
use am_engine::run::{RunOutcome, StepPlan, run_plan};
use am_store::Reader;
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

/// Every version of `document`, in version order (3.6b's reader; kept out
/// of `support/mod.rs`, which every engine and trust target includes, so
/// those targets compile before `Reader::document_versions` exists).
fn versions_of(db: &std::path::Path, document: &str) -> Vec<am_store::DocumentVersionRecord> {
    Reader::open(db)
        .unwrap()
        .document_versions(document)
        .unwrap()
}

/// Row 26: a tainted document step's version has taint 1 and so does its
/// Draft item; a clean document step's version and Draft have taint 0.
#[tokio::test]
async fn a_tainted_document_version_and_its_draft_carry_taint() {
    for (tainted, want) in [(true, Taint::Tainted), (false, Taint::Clean)] {
        let p = project();
        let summarise = documenting(
            step("summarise", texts(&["# Note\n\nOne line of summary."])),
            DOCUMENT,
        );
        let summarise = if tainted {
            poisoned(summarise)
        } else {
            summarise
        };

        let outcome = run(&p, vec![summarise]).await;

        assert_eq!(outcome.status, "succeeded", "{tainted}: {outcome:?}");
        let j = journal(&p.db);
        let attempt = j.attempt("summarise", 1);
        assert_eq!(attempt.taint, want, "{tainted}");
        let versions = versions_of(&p.db, DOCUMENT);
        assert_eq!(versions.len(), 1, "{tainted}: {versions:#?}");
        assert_eq!(versions[0].taint, want, "{tainted}: the version");
        assert_eq!(versions[0].step_run_id, attempt.id, "{tainted}");
        let drafts: Vec<_> = j
            .items
            .iter()
            .filter(|i| i.kind == InboxKind::Draft)
            .collect();
        assert_eq!(drafts.len(), 1, "{tainted}: {:#?}", j.items);
        assert_eq!(drafts[0].taint, want, "{tainted}: the Draft");
        assert_eq!(drafts[0].payload.as_deref(), Some(attempt.id.as_str()));
    }
}
