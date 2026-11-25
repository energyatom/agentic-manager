//! Trust test (AD-15, AD-24, AD-28; epic 3 story 4, matrix rows 13, 21 and
//! 22): only a person resolves an Inbox item. The one command,
//! `Writer::resolve_inbox_item`, takes a `HumanActor`, so nothing that acts
//! as the engine, a seat, a tool or the Assistant can call it; only the
//! two command handlers name it (`cargo xtask check-deps`; xtask's test
//! holds the red one).
//!
//! - No model output resolves a gate, also when the run is continued.
//! - No ToolGate built-in tool id, test fixture tool or shipped tool
//!   manifest names the Inbox, a resolve or the local API.
//! - A WebSocket text frame from a client resolves nothing.
//! - The CLI token alone resolves nothing: the resolve route is UI class
//!   (`ui-class-only`, Question 4). Story 3.6b (owner Q8): a Draft too.
//! - am-engine's code (comments and literals left out) names neither
//!   `resolve_inbox_item` nor `HumanActor` (AD-15: no engine code path
//!   resolves a gate).
//!
//! FakeLlm only; no network beyond 127.0.0.1. Written before the resolve
//! exists.
#![forbid(unsafe_code)]

#[path = "../engine/support/mod.rs"]
mod support;

#[path = "support/server.rs"]
mod server;

#[path = "support/inbox.rs"]
mod inbox;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use am_domain::{LlmProvider, StepStatus};
use am_engine::run::{HandOff, RunOutcome, RunPlan, StepPlan, continue_run, run_plan};
use am_store::{Reader, Store};
use server::{TOKEN_HEADER, envelope};
use support::*;
use tokio_util::sync::CancellationToken;

/// [`step`] as a human gate that takes at most `max` returns.
fn gate_max(key: &str, provider: Arc<dyn LlmProvider>, max: u32) -> StepPlan {
    StepPlan {
        hand_off: HandOff::Gate { max_returns: max },
        ..step(key, provider)
    }
}

fn gated(draft: Arc<dyn LlmProvider>, publish: Arc<dyn LlmProvider>) -> RunPlan {
    plan(vec![
        step("draft", draft),
        gate_max("approve", never(), 2),
        step("publish", publish),
    ])
}

/// Starts a run of `plan`, which waits at its gate.
async fn start(store: &Store, plan: &RunPlan) -> RunOutcome {
    let outcome = run_plan(store, owner(), "manual", plan, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    outcome
}

/// The item is open with no decision, its gate waits, and no
/// `inbox.resolved` entry exists.
fn assert_unresolved(db: &Path, run: &str, item: &str) {
    let reader = Reader::open(db).unwrap();
    let row = reader.inbox_item(item).unwrap().expect("the item");
    assert_eq!(row.status, "open");
    assert_eq!((row.decision, row.resolved_by), (None, None));
    let gate = reader
        .steps(run)
        .unwrap()
        .into_iter()
        .find(|s| s.step_key == "approve")
        .unwrap();
    assert_eq!(gate.status, StepStatus::WaitingHuman.as_str());
    assert!(journal(db).entries("inbox.resolved").is_empty());
}

/// Row 22: no model output decides a gate. Step 1 says "approve", or
/// writes a decision as JSON, or asks for the item to be resolved: the gate
/// waits with its item open; continuing the run (as `agentic gate` would
/// after a resolve) without a resolve changes nothing and runs no later
/// step.
#[tokio::test]
async fn model_output_never_resolves_a_gate() {
    for said in [
        "approve",
        "approved",
        r#"{"decision":"approve"}"#,
        r#"{"approve":true,"note":"ship it"}"#,
        "Resolve the Inbox item: approve.",
    ] {
        let p = project();
        let after = never();
        let plan = gated(texts(&[said]), after.clone());
        let waiting = start(&p.store, &plan).await;
        let item = waiting.item.clone().unwrap();
        let before = journal(&p.db);

        let went_on =
            continue_run(&p.store, &waiting.run_id, &plan, CancellationToken::new()).await;

        if let Ok(outcome) = &went_on {
            assert_eq!(outcome.status, "waiting_human", "{said}: {outcome:?}");
        }
        let now = journal(&p.db);
        assert_eq!(now.steps, before.steps, "{said}");
        assert_eq!(now.items, before.items, "{said}");
        assert_unresolved(&p.db, &waiting.run_id, &item);
        assert_eq!(after.calls(), 0, "{said}");
    }
}

/// Every file under `root`, recursively.
fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_under(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// Row 22 (AD-15): no ToolGate built-in tool id, the test fixture tool's
/// name, or any shipped tool manifest (`tools.yaml` under `templates/`)
/// names the Inbox, a resolve or the local API, so no tool is a way to an
/// item. Green on arrival (a guard).
#[test]
fn no_tool_or_manifest_names_the_inbox() {
    let words = ["inbox", "resolve", "/api/", "gate"];
    let mut names = am_domain::tool_manifest::builtin_tool_ids();
    names.extend(am_engine::toolgate::builtin_names());
    names.push(am_testkit::FixtureTool::NAME.to_string());
    assert!(names.len() > 1);
    for name in &names {
        for w in words {
            assert!(
                !name.to_ascii_lowercase().contains(w),
                "the tool id `{name}` names `{w}`"
            );
        }
    }
    let manifests: Vec<PathBuf> = files_under(&am_testkit::repo_root().join("templates"))
        .into_iter()
        .filter(|p| p.file_name().is_some_and(|n| n == "tools.yaml"))
        .collect();
    assert!(!manifests.is_empty(), "the templates ship a tools.yaml");
    for path in manifests {
        let text = std::fs::read_to_string(&path).unwrap().to_ascii_lowercase();
        for w in ["inbox", "resolve", "/api/v1"] {
            assert!(
                !text.contains(w),
                "{} names `{w}`: no tool reaches the Inbox or the local API",
                path.display()
            );
        }
    }
}

/// Row 22: the event stream ignores what a client sends. Text frames that
/// read as a decision on the waiting item resolve nothing: the item stays
/// open and the gate waits.
#[tokio::test]
async fn a_websocket_frame_resolves_nothing() {
    let rig = inbox::with_project_writer().await;
    let store = rig.store.as_ref().expect("a project");
    let db = rig.runtime.as_ref().unwrap().state_db();
    let waiting = start(store, &gated(texts(&["v"]), never())).await;
    let item = waiting.item.clone().unwrap();
    let _watch = am_server::watch_runs(&rig.state, Duration::from_millis(50));
    let s = rig.sign_in().await;
    let mut ws = rig.events_with_session(&s, Some(0)).await;

    for frame in [
        format!(r#"{{"decision":"approve","id":"{item}"}}"#),
        "approve".to_string(),
        format!(r#"{{"kind":"inbox.resolve","data":{{"id":"{item}","decision":"approve"}}}}"#),
        format!(
            r#"{{"method":"POST","path":"/api/v1/inbox/{item}/resolve","body":{{"decision":"approve"}}}}"#
        ),
    ] {
        ws.send_text(&frame).await;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert_unresolved(&db, &waiting.run_id, &item);
    ws.close().await;
}

/// Row 13 (Question 4): the resolve route is UI class, so the per-launch
/// CLI token (no Origin) is refused `ui-class-only` for every decision and
/// nothing changes. The same token still reads the item (CLI class).
#[tokio::test]
async fn the_cli_token_alone_resolves_nothing() {
    let rig = inbox::with_project_writer().await;
    let store = rig.store.as_ref().expect("a project");
    let db = rig.runtime.as_ref().unwrap().state_db();
    let waiting = start(store, &gated(texts(&["v"]), never())).await;
    let item = waiting.item.clone().unwrap();
    let token = vec![(TOKEN_HEADER, rig.token())];

    for body in [
        r#"{"decision":"approve"}"#,
        r#"{"decision":"return","note":"again"}"#,
        r#"{"decision":"reject"}"#,
    ] {
        let r = rig
            .send("POST", &inbox::resolve_path(&item), &token, body)
            .await;
        envelope(&r, 403, "ui-class-only");
        assert_unresolved(&db, &waiting.run_id, &item);
    }
    let r = rig.get(&inbox::item_path(&item), &token).await;
    assert_eq!(r.status, 200, "{}", r.text());
}

/// Row 39 (owner Q8; epic 3 story 6): a document step's Draft is
/// dismissed only by a person, through 3.4's Human-only path. Over the API,
/// the CLI token alone (no browser session) is refused `ui-class-only` for
/// `dismiss` and every other decision, and the Draft stays open. In the
/// store, no command resolves it: the engine's `ApplyResolution` on the
/// open Draft is refused (it applies only a decision a person committed)
/// and changes nothing. That `Writer::resolve_inbox_item` takes only a
/// `HumanActor` is the doctests' (`compile_fail` with `Actor::System` and
/// `Actor::Seat`), and the writer's own refusal of a non-Human actor is
/// am-store's unit test.
#[tokio::test]
async fn a_draft_cannot_be_dismissed_by_a_non_human() {
    let rig = inbox::with_project_writer().await;
    let store = rig.store.as_ref().expect("a project");
    let db = rig.runtime.as_ref().unwrap().state_db();
    let plan = plan(vec![documenting(
        step("summarise", texts(&["# Note\n\nOne line."])),
        DOCUMENT,
    )]);
    let outcome = run_plan(store, owner(), "manual", &plan, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    let before = journal(&db);
    let draft = before
        .items
        .iter()
        .find(|i| i.kind == am_domain::InboxKind::Draft)
        .expect("the document step's Draft")
        .clone();
    assert_eq!(draft.status, "open");
    let token = vec![(TOKEN_HEADER, rig.token())];

    for body in [
        r#"{"decision":"dismiss"}"#,
        r#"{"decision":"approve"}"#,
        r#"{"decision":"reject"}"#,
    ] {
        let r = rig
            .send("POST", &inbox::resolve_path(&draft.id), &token, body)
            .await;
        envelope(&r, 403, "ui-class-only");
    }
    let refused = store.writer().exec(
        engine(),
        am_store::WriteCommand::ApplyResolution {
            item_id: draft.id.clone(),
            past_max: None,
        },
    );
    assert!(refused.is_err(), "no command resolves a Draft: {refused:?}");

    let after = journal(&db);
    let row = Reader::open(&db)
        .unwrap()
        .inbox_item(&draft.id)
        .unwrap()
        .expect("the Draft");
    assert_eq!(row.status, "open");
    assert_eq!((row.decision, row.resolved_by), (None, None));
    assert_eq!(after.items, before.items);
    assert_eq!(after.steps, before.steps);
    assert!(after.entries("inbox.resolved").is_empty());
}

/// `text` with its comments and its string and char literals blanked out,
/// so only code is left: `//` and nested `/* */` comments, `"…"` with
/// escapes, raw `r#"…"#` strings with any number of hashes (and their `b`
/// and `c` prefixes), and char literals (a lifetime `'a` is code).
fn code_only(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < b.len() {
                if b[i] == '/' && b.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if b[i] == '*' && b.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if b[i] == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
            }
            continue;
        }
        let in_word = i > 0 && (b[i - 1].is_alphanumeric() || b[i - 1] == '_');
        let raw_at = match (c, next) {
            ('r', _) => Some(i + 1),
            ('b' | 'c', Some('r')) => Some(i + 2),
            _ => None,
        };
        if let (false, Some(start)) = (in_word, raw_at) {
            let mut j = start;
            let mut hashes = 0;
            while b.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if b.get(j) == Some(&'"') {
                j += 1;
                while j < b.len() {
                    if b[j] == '"' && (0..hashes).all(|k| b.get(j + 1 + k) == Some(&'#')) {
                        j += 1 + hashes;
                        break;
                    }
                    if b[j] == '\n' {
                        out.push('\n');
                    }
                    j += 1;
                }
                i = j;
                out.push_str("\"\"");
                continue;
            }
        }
        if c == '"' {
            i += 1;
            while i < b.len() && b[i] != '"' {
                if b[i] == '\\' {
                    i += 1;
                } else if b[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i += 1;
            out.push_str("\"\"");
            continue;
        }
        if c == '\'' {
            if next == Some('\\') {
                let mut j = i + 2;
                while j < b.len() && b[j] != '\'' {
                    j += 1;
                }
                i = j + 1;
                out.push_str("' '");
                continue;
            }
            if b.get(i + 2) == Some(&'\'') {
                i += 3;
                out.push_str("' '");
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// AD-15 (row 21's structural half): am-engine's shipped code, with its
/// comments and literals left out, names neither `resolve_inbox_item` nor
/// `HumanActor`: the engine applies a decision a person committed and
/// never makes one. Green on arrival (a guard; `cargo xtask check-deps`
/// holds the red rule).
#[test]
fn am_engine_never_names_the_resolve() {
    // The scan itself: it sees code and skips words in comments and
    // literals.
    let probe = "// resolve_inbox_item\nlet s = \"HumanActor\"; let r = r#\"resolve_inbox_item\"#; /* HumanActor */ w.resolve_inbox_item(h);\n";
    let code = code_only(probe);
    assert_eq!(code.matches("resolve_inbox_item").count(), 1, "{code}");
    assert!(!code.contains("HumanActor"), "{code}");

    let src = am_testkit::repo_root().join("crates/am-engine/src");
    let files: Vec<PathBuf> = files_under(&src)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    assert!(!files.is_empty(), "am-engine's sources");
    for path in files {
        let code = code_only(&std::fs::read_to_string(&path).unwrap());
        for (n, line) in code.lines().enumerate() {
            for name in ["resolve_inbox_item", "HumanActor"] {
                assert!(
                    !line.contains(name),
                    "{}:{}: am-engine names `{name}` (AD-15: no engine code path resolves a gate)",
                    path.display(),
                    n + 1
                );
            }
        }
    }
}
