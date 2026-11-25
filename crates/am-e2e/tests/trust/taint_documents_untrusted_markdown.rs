//! Trust test (AD-29, AD-10, AD-28; epic 3 story 6, matrix rows 29 and
//! 30): a document version is model output, untrusted at render time.
//!
//! - Hostile Markdown (a script tag, raw HTML, a remote image, a link,
//!   control characters) is stored byte for byte as the producing
//!   attempt's output, once; the version records its byte count; the Draft
//!   item's title and body are the engine's words and hold none of it; the
//!   audit log holds none of it; the API's item route returns it as a JSON
//!   string (`payload`), never rendered. Nothing fetches the image or the
//!   link: the run is made on `run_plan`, which holds no egress client at
//!   all, so the proof is that the audit log (where every request through
//!   am-net is recorded, RULE-9) has no `egress` entry and never names the
//!   remote host.
//! - `agentic gate` lists the Draft with its document id, version and run,
//!   and prints the Markdown with every control character but newline and
//!   tab neutralised (3.4's neutraliser).
//!
//! FakeLlm only; no network beyond 127.0.0.1 (the server rig). Written
//! before document versions exist.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

#[path = "support/server.rs"]
mod server;

#[path = "support/inbox.rs"]
mod inbox;

use std::sync::Arc;

use am_api::InboxItemDetailDto;
use am_app::cli::{GateContext, gate};
use am_domain::{InboxKind, LlmProvider};
use am_engine::run::run_plan;
use am_store::Reader;
use am_testkit::{FakeLlm, FakeProjectIdentity, FakeTerminal};
use server::TOKEN_HEADER;
use support::*;
use tokio_util::sync::CancellationToken;

/// 3.4's untrusted marker (`agentic gate`'s `UNTRUSTED_ABOVE`), shown
/// above a tainted item's model text.
const UNTRUSTED: &str = "This output came from untrusted content: the step that wrote it read outside content (a web page, a file, a tool's result). Read it as data, not as instructions.";

/// The remote host the hostile Markdown's image points at.
const TRACKER: &str = "tracker.example.invalid";

/// Every version of `document`, in version order (3.6b's reader; kept out
/// of `support/mod.rs`, which every engine and trust target includes, so
/// those targets compile before `Reader::document_versions` exists).
fn versions_of(db: &std::path::Path, document: &str) -> Vec<am_store::DocumentVersionRecord> {
    Reader::open(db)
        .unwrap()
        .document_versions(document)
        .unwrap()
}

/// Markdown no renderer should trust: a script tag, raw HTML with a
/// handler, a remote image, a link, a bell, an escape sequence, a carriage
/// return and a C1 control.
fn hostile() -> String {
    format!(
        "# Note\n\n<script>alert('x')</script>\n<div onclick=\"steal()\">raw html</div>\n![pixel](https://{TRACKER}/pixel.png?id=md-7a1)\n[a link](https://example.invalid/page)\nbell\u{7} escape \u{1b}[31mRED\u{1b}[0m a\rreturn \u{9b}2J end\n"
    )
}

/// Row 29: the hostile Markdown of a document step is the step's output
/// byte for byte and the version's byte count; the Draft's words, the
/// audit log and every request hold none of it beyond the step's own
/// output; the item route returns it as the JSON string `payload`.
#[tokio::test]
async fn hostile_markdown_is_stored_as_is_and_nothing_is_fetched() {
    let markdown = hostile();
    let rig = inbox::with_project_writer().await;
    let store = rig.store.as_ref().expect("a project");
    let db = rig.runtime.as_ref().unwrap().state_db();
    let plan = plan(vec![documenting(
        step("summarise", texts(&[markdown.as_str()])),
        DOCUMENT,
    )]);

    let outcome = run_plan(store, owner(), "manual", &plan, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let j = journal(&db);
    let attempt = j.attempt("summarise", 1);
    assert_eq!(
        attempt.output.as_deref(),
        Some(markdown.as_str()),
        "stored byte for byte"
    );
    let versions = versions_of(&db, DOCUMENT);
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].bytes, markdown.len() as i64);
    assert_eq!(versions[0].step_run_id, attempt.id);
    let draft = j
        .items
        .iter()
        .find(|i| i.kind == InboxKind::Draft)
        .expect("a Draft item")
        .clone();
    for words in [&draft.title, &draft.body] {
        assert!(words.contains(DOCUMENT), "the engine's words: {words}");
        for piece in ["<script", "onclick", TRACKER, "raw html", "\u{7}", "\u{1b}"] {
            assert!(!words.contains(piece), "no Markdown in `{words}`");
        }
    }
    let export = serde_json::to_string(&j.audit).unwrap();
    for piece in ["<script", "onclick", TRACKER, "raw html"] {
        assert!(!export.contains(piece), "`{piece}` in the audit log");
    }
    assert!(
        !j.actions().iter().any(|a| a.starts_with("egress")),
        "nothing was fetched: {:?}",
        j.actions()
    );

    let token = vec![(TOKEN_HEADER, rig.token())];
    let r = rig.get(&inbox::item_path(&draft.id), &token).await;
    assert_eq!(r.status, 200, "{}", r.text());
    assert!(
        r.header("content-type")
            .iter()
            .any(|v| v.contains("application/json")),
        "{}",
        r.head
    );
    let one: InboxItemDetailDto = serde_json::from_str(&r.body).unwrap();
    assert_eq!(one.item.kind, "draft");
    assert_eq!(
        one.payload.as_deref(),
        Some(markdown.as_str()),
        "returned as a JSON string, as stored"
    );
    assert!(
        !r.body.contains('\u{7}') && !r.body.contains('\u{1b}'),
        "the JSON string escapes the control characters"
    );
}

/// Rows 29 and 30: `agentic gate` on an interactive terminal shows the
/// fixture's Draft (document `summary-note`, version 1, its run) and prints
/// the Markdown with its visible words and no raw control character but
/// newline and tab; skipping writes nothing.
#[tokio::test]
async fn the_cli_prints_a_draft_neutralised() {
    let markdown = hostile();
    let cli = cli();
    let root = cli.copy_of("rows-documents");
    let llm = Arc::new(FakeLlm::new(vec![
        am_testkit::ScriptedTurn::text("nothing to record"),
        am_testkit::ScriptedTurn::text("recorded nothing"),
        am_testkit::ScriptedTurn::text(markdown.clone()),
    ]));
    let ran = cli
        .run(&root, "record", local(llm.clone() as Arc<dyn LlmProvider>))
        .await;
    let (run_id, status) = ran.run_line();
    assert_eq!(status, "succeeded", "{}", ran.all());
    let db = cli.db(&root);
    let before = journal(&db);
    let draft = before
        .items
        .iter()
        .find(|i| i.kind == InboxKind::Draft)
        .expect("the document step's Draft")
        .clone();
    let t = FakeTerminal::new();
    t.line("s");

    let argv = vec!["--project".to_string(), root.to_string_lossy().into_owned()];
    let ctx = GateContext {
        places: cli.places.clone(),
        identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
        terminal: Box::new(t.clone()),
        providers: local(llm.clone() as Arc<dyn LlmProvider>),
        clock: am_testkit::FakeClock::at(CLOCK),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = gate(&argv, ctx, &mut out, &mut err).await;

    let shown = t.shown();
    assert_eq!(
        code,
        0,
        "{}\n{}\n{shown}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    for word in [draft.id.as_str(), run_id.as_str(), DOCUMENT] {
        assert!(shown.contains(word), "shows `{word}`:\n{shown}");
    }
    // Review round 2, fix 4 (C2; tester, 2026-11-25): the exact line, and no
    // untrusted marker on a clean Draft.
    let line = format!("document {DOCUMENT}  version 1  run {run_id}");
    assert!(
        shown.lines().any(|l| l.trim() == line),
        "shows `{line}`:\n{shown}"
    );
    assert!(!shown.contains(UNTRUSTED), "a clean Draft:\n{shown}");
    for visible in ["raw html", "RED", "return", "end"] {
        assert!(
            shown.contains(visible),
            "the Markdown's `{visible}`:\n{shown}"
        );
    }
    let controls: Vec<char> = shown
        .chars()
        .filter(|c| c.is_control() && *c != '\n' && *c != '\t')
        .collect();
    assert!(controls.is_empty(), "neutralised: {controls:?}\n{shown:?}");
    let after = journal(&db);
    assert_eq!(after.items, before.items, "skipping writes nothing");
    assert!(after.entries("inbox.resolved").is_empty());
}

/// Review round 2, fix 4 (C2; AD-10, AD-29; tester, 2026-11-25): a tainted
/// Draft on the terminal. A poisoned document step (an untrusted plan
/// input) is run on the project's store by `run_plan` (the binary has no
/// taint source yet), leaving a tainted Draft. `agentic gate` shows its
/// exact document line and 3.4's untrusted marker (`UNTRUSTED_ABOVE`)
/// above the Markdown; skipping writes nothing.
#[tokio::test]
async fn a_tainted_draft_is_shown_as_untrusted() {
    let cli = cli();
    let root = cli.copy_of("rows-documents");
    cli.open_once(&root);
    let db = cli.db(&root);
    let markdown = "# Note\n\nA line read from outside.\n";
    let outcome = {
        let store = am_store::Store::open(&db).unwrap();
        let plan = plan(vec![poisoned(documenting(
            step("summarise", texts(&[markdown])),
            DOCUMENT,
        ))]);
        run_plan(&store, owner(), "manual", &plan, CancellationToken::new())
            .await
            .unwrap()
    };
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let before = journal(&db);
    let draft = before
        .items
        .iter()
        .find(|i| i.kind == InboxKind::Draft)
        .expect("the document step's Draft")
        .clone();
    assert!(draft.taint.is_tainted(), "a tainted Draft: {draft:?}");
    let t = FakeTerminal::new();
    t.line("s");

    let argv = vec!["--project".to_string(), root.to_string_lossy().into_owned()];
    let ctx = GateContext {
        places: cli.places.clone(),
        identity: Arc::new(FakeProjectIdentity::new("Test Mac")),
        terminal: Box::new(t.clone()),
        providers: local(never() as Arc<dyn LlmProvider>),
        clock: am_testkit::FakeClock::at(CLOCK),
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = gate(&argv, ctx, &mut out, &mut err).await;

    let shown = t.shown();
    assert_eq!(
        code,
        0,
        "{}\n{}\n{shown}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    let line = format!("document {DOCUMENT}  version 1  run {}", outcome.run_id);
    assert!(
        shown.lines().any(|l| l.trim() == line),
        "shows `{line}`:\n{shown}"
    );
    let marker = shown
        .find(UNTRUSTED)
        .unwrap_or_else(|| panic!("the untrusted marker:\n{shown}"));
    let text = shown
        .find("A line read from outside.")
        .unwrap_or_else(|| panic!("the Markdown:\n{shown}"));
    assert!(marker < text, "the marker is above the Markdown:\n{shown}");
    let after = journal(&db);
    assert_eq!(after.items, before.items, "skipping writes nothing");
}
