//! RULE-15 trust test (epic 3 story 4; AD-13, AD-14, AD-15, AD-28): the
//! Inbox over `/api/v1` on 3.2's server, which holds the open project's
//! writer (`AppStateBuilder::project_writer`).
//!
//! - `GET /api/v1/inbox` (open items) and `GET /api/v1/inbox/{id}` (one
//!   item, with its payload's text and taint) are CLI class: the session or
//!   the CLI token with no Origin.
//! - `POST /api/v1/inbox/{id}/resolve {decision, note?, confirmItemId?}` is
//!   UI class (session cookie, CSRF value, matching Origin; Question 4) and
//!   never a GET, HEAD, PUT or DELETE. It records the decision as You
//!   (Reviewer) through the one Human-only command, then the server applies
//!   it as `System(engine)`; it runs no model step. A tainted approve needs
//!   `confirmItemId` equal to the item's id (owner condition 2: the API is
//!   not a way around the CLI's typed id), else 422 `confirm-required`. A
//!   second resolve is 409 `item-resolved`; a decision that does not fit
//!   is 422 `decision-not-allowed`; a return without its note 422
//!   `note-required`. Every refusal is AD-14's envelope and changes nothing.
//! - Each Inbox change is one `inbox.updated` event whose data is exactly
//!   `{id, status}` (owner condition 4): never the payload, title, body,
//!   decision, note or run; the watcher's first look publishes nothing old.
//!
//! FakeLlm only; no network beyond 127.0.0.1. Written before the routes
//! exist.
#![forbid(unsafe_code)]

#[path = "support/server.rs"]
mod server;

#[path = "support/inbox.rs"]
mod inbox;

#[path = "../engine/support/mod.rs"]
mod support;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use am_api::{
    EVENT_INBOX_UPDATED, EventDto, InboxItemDetailDto, InboxItemDto, InboxUpdatedDto,
    ResolutionDto, ResolveBody,
};
use am_domain::{Decision, HumanActor, InboxKind, LlmProvider, REVIEWER_HAT, Taint};
use am_engine::run::{HandOff, RunOutcome, RunPlan, StepPlan, run_plan};
use am_store::{NewInboxItem, StepInput, Store, WriteCommand, new_id};
use inbox::{
    FakePin, INBOX, OTHER, PINNED, events_until, item_path, resolve, resolve_path, with_pin,
    with_project_writer,
};
use server::{EVIL, Resp, Rig, TOKEN_HEADER, assert_no_cors, envelope};
use support::{Journal, journal, never, owner, plan, poisoned, step, texts};
use tokio_util::sync::CancellationToken;

const DRAFT_1: &str = "the version to review";
const NOTE: &str = "tighten the summary";
const APPROVE: &str = r#"{"decision":"approve"}"#;
const REJECT: &str = r#"{"decision":"reject"}"#;

fn return_with(note: &str) -> String {
    serde_json::json!({"decision": "return", "note": note}).to_string()
}

fn gate_max(key: &str, provider: Arc<dyn LlmProvider>, max: u32) -> StepPlan {
    StepPlan {
        hand_off: HandOff::Gate { max_returns: max },
        ..step(key, provider)
    }
}

fn gated(draft: Arc<dyn LlmProvider>) -> RunPlan {
    plan(vec![
        step("draft", draft),
        gate_max("approve", never(), 2),
        step("publish", never()),
    ])
}

/// A run of the fixture's shape on the rig's store, waiting at its gate.
async fn waiting(rig: &Rig, plan: &RunPlan) -> RunOutcome {
    let outcome = run_plan(
        rig.store.as_ref().expect("a project"),
        owner(),
        "manual",
        plan,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(outcome.status, "waiting_human", "{outcome:?}");
    outcome
}

fn db(rig: &Rig) -> PathBuf {
    rig.runtime.as_ref().unwrap().state_db()
}

/// What a refused request must leave as it was.
fn unchanged(before: &Journal, rig: &Rig) {
    let now = journal(&db(rig));
    assert_eq!(now.steps, before.steps);
    assert_eq!(now.items, before.items);
    assert_eq!(now.inputs, before.inputs);
    assert_eq!(
        now.entries("inbox.resolved").len(),
        before.entries("inbox.resolved").len()
    );
}

/// Row 12: the session and the CLI token (no Origin) both read the Inbox:
/// the list holds the open items only (a resolved one is not listed),
/// camelCase; one item adds its payload's text and taint; a resolved item
/// read by id shows its decision and "You (Reviewer)"; an unknown id is 404
/// `not-found`; with no project open both are 409 `no-project`.
#[tokio::test]
async fn the_inbox_reads_with_the_cli_token_or_a_session() {
    let rig = with_project_writer().await;
    let open = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
    let item = open.item.clone().unwrap();
    let done = waiting(&rig, &gated(texts(&["another version"]))).await;
    let resolved = done.item.clone().unwrap();
    rig.store
        .as_ref()
        .unwrap()
        .writer()
        .resolve_inbox_item(
            HumanActor::new("owner", REVIEWER_HAT),
            &resolved,
            Decision::Reject,
            None,
        )
        .unwrap();
    let s = rig.sign_in().await;

    for h in [s.auth(), vec![(TOKEN_HEADER, rig.token())]] {
        let r = rig.get(INBOX, &h).await;
        assert_eq!(r.status, 200, "{}", r.text());
        assert_no_cors(&r);
        assert!(r.body.contains("\"runId\""), "camelCase: {}", r.body);
        let list: Vec<InboxItemDto> = serde_json::from_str(&r.body).unwrap();
        let ids: Vec<&str> = list.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec![item.as_str()], "open items only");
        let it = &list[0];
        assert_eq!(
            (it.kind.as_str(), it.status.as_str(), it.run_id.as_str()),
            ("review", "open", open.run_id.as_str())
        );
        assert!(!it.taint);
        assert_eq!(it.max_returns, Some(2));
        assert_eq!((it.decision.clone(), it.resolved_by.clone()), (None, None));

        let r = rig.get(&item_path(&item), &h).await;
        assert_eq!(r.status, 200, "{}", r.text());
        let one: InboxItemDetailDto = serde_json::from_str(&r.body).unwrap();
        assert_eq!(one.item.id, item);
        assert_eq!(one.payload.as_deref(), Some(DRAFT_1));
        assert!(!one.item.taint);
        assert_eq!(one.note, None);

        let r = rig.get(&item_path(&resolved), &h).await;
        assert_eq!(r.status, 200, "{}", r.text());
        let one: InboxItemDetailDto = serde_json::from_str(&r.body).unwrap();
        assert_eq!(one.item.status, "resolved");
        assert_eq!(one.item.decision.as_deref(), Some("reject"));
        assert_eq!(one.item.resolved_by.as_deref(), Some("You (Reviewer)"));

        envelope(&rig.get(&item_path(&new_id()), &h).await, 404, "not-found");
    }

    let bare = Rig::start().await;
    let s = bare.sign_in().await;
    envelope(&bare.get(INBOX, &s.auth()).await, 409, "no-project");
    envelope(
        &bare.get(&item_path(&new_id()), &s.auth()).await,
        409,
        "no-project",
    );
}

/// Row 14 (RULE-15 on resolve): the cookie alone (or a wrong CSRF value)
/// is `csrf-required`, the cookie and CSRF without an Origin
/// `origin-required`, a foreign Origin `origin-not-allowed`; nothing
/// changes. With all three the resolve goes through.
#[tokio::test]
async fn resolve_needs_the_session_csrf_and_a_matching_origin() {
    let rig = with_project_writer().await;
    let run = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
    let item = run.item.clone().unwrap();
    let path = resolve_path(&item);
    let s = rig.sign_in().await;
    let before = journal(&db(&rig));

    let mut cookie_only = s.cookie_only();
    cookie_only.push(("Origin", rig.origin()));
    envelope(
        &rig.send("POST", &path, &cookie_only, APPROVE).await,
        403,
        "csrf-required",
    );
    let wrong = vec![
        ("Cookie", s.cookie.clone()),
        ("X-AM-CSRF", "0".repeat(64)),
        ("Origin", rig.origin()),
    ];
    envelope(
        &rig.send("POST", &path, &wrong, APPROVE).await,
        403,
        "csrf-required",
    );
    envelope(
        &rig.send("POST", &path, &s.auth(), APPROVE).await,
        403,
        "origin-required",
    );
    let mut foreign = s.auth();
    foreign.push(("Origin", EVIL.to_string()));
    envelope(
        &rig.send("POST", &path, &foreign, APPROVE).await,
        403,
        "origin-not-allowed",
    );
    unchanged(&before, &rig);

    let r = resolve(&rig, &s, &item, APPROVE).await;
    assert_eq!(r.status, 200, "{}", r.text());
    assert_no_cors(&r);
}

/// Row 14: the resolve route changes state, so GET, HEAD, PUT, DELETE and
/// PATCH to it are 405 `method-not-allowed` (HEAD with no body), with a
/// full session, and nothing changes.
#[tokio::test]
async fn resolve_is_never_a_get_head_put_or_delete() {
    let rig = with_project_writer().await;
    let run = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
    let item = run.item.clone().unwrap();
    let path = resolve_path(&item);
    let s = rig.sign_in().await;
    let mut h = s.auth();
    h.push(("Origin", rig.origin()));
    let before = journal(&db(&rig));

    envelope(&rig.get(&path, &h).await, 405, "method-not-allowed");
    for m in ["PUT", "DELETE", "PATCH"] {
        envelope(
            &rig.send(m, &path, &h, APPROVE).await,
            405,
            "method-not-allowed",
        );
    }
    let head = rig.send("HEAD", &path, &h, "").await;
    assert_eq!(head.status, 405, "{}", head.text());
    assert_no_cors(&head);
    unchanged(&before, &rig);
}

/// Rows 10 and 11: approve, return (with its note) and reject over the
/// API, each on a run waiting at its gate: 200 with the resolved item
/// (`resolvedBy` "You (Reviewer)") and the run; the server applied the
/// decision as `System(engine)` (approve: the gate `succeeded`, `publish`
/// queued, the run `running`; return: the gate `looped_back`, the next
/// attempts queued with one `gate_note` input, the note on the item;
/// reject: the gate and the run `failed`, no Failure item). One
/// `inbox.resolved` entry by You (Reviewer) with the kind, decision, step
/// run, taint and the note's sha256, never the note; every later entry is
/// the engine's; the chain verifies.
#[tokio::test]
async fn the_same_resolve_over_the_api_is_audited_you_reviewer() {
    let returned = return_with(NOTE);
    for (decision, body) in [
        (Decision::Approve, APPROVE),
        (Decision::Return, returned.as_str()),
        (Decision::Reject, REJECT),
    ] {
        let rig = with_project_writer().await;
        let run = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
        let item = run.item.clone().unwrap();
        let s = rig.sign_in().await;

        let r = resolve(&rig, &s, &item, body).await;

        assert_eq!(r.status, 200, "{decision:?}: {}", r.text());
        let done: ResolutionDto = serde_json::from_str(&r.body).unwrap();
        assert!(done.applied, "the server applied it (review R2)");
        assert_eq!(done.item.id, item);
        assert_eq!(done.item.status, "resolved");
        assert_eq!(done.item.decision.as_deref(), Some(decision.as_str()));
        assert_eq!(done.item.resolved_by.as_deref(), Some("You (Reviewer)"));
        assert_eq!(done.run.id, run.run_id);
        let j = journal(&db(&rig));
        let gate = j.attempt("approve", 1);
        match decision {
            Decision::Approve => {
                assert_eq!(gate.status, "succeeded");
                assert_eq!(j.attempt("publish", 1).status, "queued");
                assert_eq!(done.run.status, "running");
            }
            Decision::Return => {
                assert_eq!(gate.status, "looped_back");
                assert_eq!(j.attempt("draft", 2).status, "queued");
                assert_eq!(j.attempt("approve", 2).status, "queued");
                assert_eq!(
                    j.inputs_of(&j.attempt("draft", 2).id),
                    vec![StepInput {
                        kind: "gate_note".into(),
                        from_step_run: None,
                        from_item: Some(item.clone()),
                        label: "gate-note:approve@1".into(),
                        taint: Taint::Clean,
                    }]
                );
                assert_eq!(done.run.status, "running");
            }
            _ => {
                assert_eq!(gate.status, "failed");
                assert_eq!(j.attempt("publish", 1).status, "queued");
                assert_eq!(done.run.status, "failed");
                assert!(!j.items.iter().any(|i| i.kind == InboxKind::Failure));
            }
        }
        let review = j.items.iter().find(|i| i.id == item).unwrap();
        assert_eq!(review.decision, Some(decision));
        let resolved = j.entries("inbox.resolved");
        assert_eq!(resolved.len(), 1, "{:#?}", j.audit);
        let e = &resolved[0];
        assert_eq!(e["target"], item.as_str());
        assert_eq!(
            e["actor"],
            serde_json::json!({"kind": "human", "person": "owner", "hat": "Reviewer"})
        );
        assert_eq!(e["args"]["kind"], "review");
        assert_eq!(e["args"]["decision"], decision.as_str());
        assert_eq!(e["args"]["step_run_id"], gate.id.as_str());
        let note_hash = e["args"]["note_sha256"].clone();
        if decision == Decision::Return {
            assert_eq!(
                note_hash,
                serde_json::Value::from(hex::encode(<sha2::Sha256 as sha2::Digest>::digest(
                    NOTE.as_bytes()
                )))
            );
        } else {
            assert!(note_hash.is_null(), "{e}");
        }
        let at = j
            .audit
            .iter()
            .position(|e| e["action"] == "inbox.resolved")
            .unwrap();
        assert!(j.audit.len() > at + 1, "the apply is audited");
        for later in &j.audit[at + 1..] {
            assert_eq!(later["actor"]["kind"], "system", "{later}");
            assert_eq!(later["actor"]["component"], "engine", "{later}");
        }
        let all = serde_json::to_string(&j.audit).unwrap();
        assert!(!all.contains(NOTE), "never the note in the audit log");
        assert_eq!(
            rig.store
                .as_ref()
                .unwrap()
                .reader()
                .unwrap()
                .verify_audit_chain()
                .unwrap(),
            None
        );
    }
}

/// Row 40: two resolves of one item at once (approve and reject): one is
/// 200, the other 409 `item-resolved`; one `inbox.resolved` entry; the item
/// holds the winner's decision. A later resolve is 409 too.
#[tokio::test]
async fn a_second_resolve_is_refused() {
    let rig = with_project_writer().await;
    let run = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
    let item = run.item.clone().unwrap();
    let s = rig.sign_in().await;

    let (a, b) = tokio::join!(
        resolve(&rig, &s, &item, APPROVE),
        resolve(&rig, &s, &item, REJECT)
    );

    let mut statuses = [a.status, b.status];
    statuses.sort();
    assert_eq!(statuses, [200, 409], "{}\n{}", a.text(), b.text());
    let (won, lost, decision) = if a.status == 200 {
        (&a, &b, Decision::Approve)
    } else {
        (&b, &a, Decision::Reject)
    };
    assert_eq!(won.status, 200);
    envelope(lost, 409, "item-resolved");
    let j = journal(&db(&rig));
    assert_eq!(j.entries("inbox.resolved").len(), 1);
    assert_eq!(
        j.items.iter().find(|i| i.id == item).unwrap().decision,
        Some(decision)
    );
    let before = journal(&db(&rig));
    envelope(
        &resolve(&rig, &s, &item, &return_with(NOTE)).await,
        409,
        "item-resolved",
    );
    unchanged(&before, &rig);
}

/// Decision 6: a decision that does not fit the item is 422
/// `decision-not-allowed` (dismiss or answer on a Review), a return with
/// no note or an empty one 422 `note-required`, an unknown item 404
/// `not-found`; nothing changes.
#[tokio::test]
async fn a_resolve_that_does_not_fit_is_refused_in_the_envelope() {
    let rig = with_project_writer().await;
    let run = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
    let item = run.item.clone().unwrap();
    let s = rig.sign_in().await;
    let before = journal(&db(&rig));

    for body in [
        r#"{"decision":"dismiss"}"#.to_string(),
        serde_json::json!({"decision": "answer", "note": "an answer"}).to_string(),
    ] {
        envelope(
            &resolve(&rig, &s, &item, &body).await,
            422,
            "decision-not-allowed",
        );
    }
    for body in [r#"{"decision":"return"}"#.to_string(), return_with("")] {
        envelope(&resolve(&rig, &s, &item, &body).await, 422, "note-required");
    }
    envelope(
        &resolve(&rig, &s, &new_id(), APPROVE).await,
        404,
        "not-found",
    );
    unchanged(&before, &rig);
}

/// The `inbox.updated` events in `seen` for `item`.
fn inbox_events<'a>(seen: &'a [EventDto], item: &str) -> Vec<&'a EventDto> {
    seen.iter()
        .filter(|e| e.kind == EVENT_INBOX_UPDATED && e.data["id"] == item)
        .collect()
}

/// The data of an `inbox.updated` event has exactly two keys, `id` and
/// `status`, and reads as `am_api::InboxUpdatedDto`.
fn assert_id_and_status_only(e: &EventDto) {
    let keys: BTreeSet<&str> = e
        .data
        .as_object()
        .unwrap_or_else(|| panic!("an object: {e:?}"))
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, BTreeSet::from(["id", "status"]), "{e:?}");
    let dto: InboxUpdatedDto = serde_json::from_value(e.data.clone()).unwrap();
    assert_eq!(serde_json::to_value(&dto).unwrap(), e.data);
}

/// Row 39: under the run watcher, an item made (the gate pausing) and
/// then resolved over the API each publish one `inbox.updated` event, the
/// first `open`, the second `resolved`, and no other for that item; each
/// event's data is only the id and the status (no decision, note, run,
/// kind, title, body or payload).
#[tokio::test]
async fn inbox_changes_are_published_once_without_words() {
    let rig = with_project_writer().await;
    let _watch = am_server::watch_runs(&rig.state, Duration::from_millis(50));
    let s = rig.sign_in().await;
    let mut ws = rig.events_with_session(&s, Some(0)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let run = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
    let item = run.item.clone().unwrap();
    let mut seen = Vec::new();
    events_until(&mut ws, &mut seen, |e| {
        e.kind == EVENT_INBOX_UPDATED && e.data["id"] == item.as_str() && e.data["status"] == "open"
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    let r = resolve(&rig, &s, &item, &return_with(NOTE)).await;
    assert_eq!(r.status, 200, "{}", r.text());
    events_until(&mut ws, &mut seen, |e| {
        e.kind == EVENT_INBOX_UPDATED
            && e.data["id"] == item.as_str()
            && e.data["status"] == "resolved"
    })
    .await;
    seen.extend(ws.take(50, Duration::from_millis(700)).await);

    let mine = inbox_events(&seen, &item);
    let statuses: Vec<&str> = mine
        .iter()
        .map(|e| e.data["status"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        statuses,
        vec!["open", "resolved"],
        "one per change: {seen:#?}"
    );
    for e in mine {
        assert_id_and_status_only(e);
    }
    ws.close().await;
}

/// Row 39 (owner condition 4): canary strings planted in an item's payload
/// (the producing step's output), title and body (made by a System pause
/// here) and in the return note appear in no frame of the event stream,
/// while the item, its pause and its resolve are published; every
/// `inbox.updated` event's data has exactly the keys `id` and `status`.
/// The canaries are real data: the item's read route returns the payload's.
#[tokio::test]
async fn an_inbox_event_carries_only_the_id_and_status() {
    const PAYLOAD: &str = "canary-payload-31c7 the draft text";
    const TITLE: &str = "canary-title-8e02 Review this";
    const BODY: &str = "canary-body-55af the engine's words";
    const CANARY_NOTE: &str = "canary-note-c9d4 tighten it";
    let rig = with_project_writer().await;
    let _watch = am_server::watch_runs(&rig.state, Duration::from_millis(50));
    let s = rig.sign_in().await;
    let mut ws = rig.events_with_session(&s, Some(0)).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // The run, by System commands: `draft` writes the payload, `approve`
    // waits with the canary-worded Review item.
    let store: &Store = rig.store.as_ref().unwrap();
    let w = store.writer();
    let sys = || am_domain::Actor::System {
        component: "engine".into(),
    };
    let run = new_id();
    w.exec(
        sys(),
        WriteCommand::CreateRun {
            run_id: run.clone(),
            workflow: "wf".into(),
            origin: "manual".into(),
            steps: vec!["draft".into(), "approve".into(), "publish".into()],
            config_hash: Some(PINNED.into()),
        },
    )
    .unwrap();
    let steps = store.reader().unwrap().steps(&run).unwrap();
    let (draft, approve) = (steps[0].id.clone(), steps[1].id.clone());
    let call = new_id();
    w.exec(
        sys(),
        WriteCommand::StartAttempt {
            step_run_id: draft.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::ReserveCall {
            step_run_id: draft.clone(),
            call_id: call.clone(),
            call_seq: 1,
            reserved_micro_usd: 0,
            rate_snapshot: "none".into(),
            taint: Taint::Clean,
            purpose: am_store::CallPurpose::Turn,
            prompt: vec![],
        },
    )
    .unwrap();
    w.exec(
        sys(),
        WriteCommand::SettleAndFinishStep {
            call_id: call,
            usage: None,
            cost_micro_usd: None,
            step_run_id: draft.clone(),
            status: "succeeded".into(),
            output: Some(PAYLOAD.into()),
        },
    )
    .unwrap();
    let item = new_id();
    w.exec(
        sys(),
        WriteCommand::PauseStep {
            step_run_id: approve,
            inputs: vec![StepInput {
                kind: "step_output".into(),
                from_step_run: Some(draft.clone()),
                from_item: None,
                label: "step:draft@1".into(),
                taint: Taint::Clean,
            }],
            taint: Taint::Clean,
            item: NewInboxItem {
                id: item.clone(),
                kind: InboxKind::Review,
                payload: Some(draft),
                taint: Taint::Clean,
                title: TITLE.into(),
                body: BODY.into(),
                max_returns: Some(2),
            },
        },
    )
    .unwrap();
    let mut seen = Vec::new();
    events_until(&mut ws, &mut seen, |e| {
        e.kind == EVENT_INBOX_UPDATED && e.data["id"] == item.as_str()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let read = rig
        .get(&item_path(&item), &[(TOKEN_HEADER, rig.token())])
        .await;
    assert!(read.body.contains(PAYLOAD), "{}", read.text());

    let r = resolve(&rig, &s, &item, &return_with(CANARY_NOTE)).await;
    assert_eq!(r.status, 200, "{}", r.text());
    events_until(&mut ws, &mut seen, |e| {
        e.kind == EVENT_INBOX_UPDATED
            && e.data["id"] == item.as_str()
            && e.data["status"] == "resolved"
    })
    .await;
    seen.extend(ws.take(50, Duration::from_millis(700)).await);

    assert!(
        seen.iter().any(|e| e.kind == "step.updated"),
        "the steps' changes are published too: {seen:#?}"
    );
    for e in &seen {
        let frame = serde_json::to_string(e).unwrap();
        for canary in [PAYLOAD, TITLE, BODY, CANARY_NOTE] {
            assert!(!frame.contains(canary), "{canary} in a frame: {frame}");
        }
        if e.kind == EVENT_INBOX_UPDATED {
            assert_id_and_status_only(e);
        }
    }
    ws.close().await;
}

/// Row 27a (owner condition 2): a tainted Review's approve needs
/// `confirmItemId` equal to the item's id. Without it, empty, a prefix,
/// another id or the id in capitals: 422 `confirm-required`, nothing
/// changes. With it: 200. A tainted return needs no id.
#[tokio::test]
async fn a_tainted_approve_over_the_api_needs_the_item_id() {
    let rig = with_project_writer().await;
    let tainted = || {
        plan(vec![
            poisoned(step("draft", texts(&["a version that read a page"]))),
            gate_max("approve", never(), 2),
            step("publish", never()),
        ])
    };
    let run = waiting(&rig, &tainted()).await;
    let item = run.item.clone().unwrap();
    let s = rig.sign_in().await;
    let j = journal(&db(&rig));
    assert_eq!(
        j.items.iter().find(|i| i.id == item).unwrap().taint,
        Taint::Tainted
    );
    let before = journal(&db(&rig));

    let confirm = |id: Option<String>| {
        serde_json::to_string(&ResolveBody {
            decision: "approve".into(),
            note: None,
            confirm_item_id: id,
        })
        .unwrap()
    };
    for (what, body) in [
        ("no confirmItemId", APPROVE.to_string()),
        ("an empty one", confirm(Some(String::new()))),
        ("a prefix", confirm(Some(item[..8].to_string()))),
        ("another id", confirm(Some(new_id()))),
        ("the id in capitals", confirm(Some(item.to_uppercase()))),
    ] {
        let r = resolve(&rig, &s, &item, &body).await;
        assert_eq!(r.status, 422, "{what}: {}", r.text());
        envelope(&r, 422, "confirm-required");
        unchanged(&before, &rig);
    }

    let r = resolve(&rig, &s, &item, &confirm(Some(item.clone()))).await;
    assert_eq!(r.status, 200, "{}", r.text());
    let done: ResolutionDto = serde_json::from_str(&r.body).unwrap();
    assert_eq!(done.item.decision.as_deref(), Some("approve"));
    assert!(done.item.taint, "resolving keeps the taint");

    let other = waiting(&rig, &tainted()).await;
    let other_item = other.item.clone().unwrap();
    let r = resolve(&rig, &s, &other_item, &return_with(NOTE)).await;
    assert_eq!(r.status, 200, "a tainted return needs no id: {}", r.text());
}

/// Row 39: a Review item that exists before the watcher starts is not
/// published by its first look; its later resolve is, once.
#[tokio::test]
async fn the_first_look_publishes_no_old_item() {
    let rig = with_project_writer().await;
    let run = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
    let item = run.item.clone().unwrap();
    let _watch = am_server::watch_runs(&rig.state, Duration::from_millis(50));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let s = rig.sign_in().await;
    let mut ws = rig.events_with_session(&s, Some(0)).await;

    let first: Vec<EventDto> = ws.take(10, Duration::from_millis(500)).await;
    assert!(
        !first.iter().any(|e| e.kind == EVENT_INBOX_UPDATED),
        "the first look publishes nothing old: {first:#?}"
    );

    let r: Resp = resolve(&rig, &s, &item, APPROVE).await;
    assert_eq!(r.status, 200, "{}", r.text());
    let mut seen = first;
    events_until(&mut ws, &mut seen, |e| {
        e.kind == EVENT_INBOX_UPDATED && e.data["id"] == item.as_str()
    })
    .await;
    seen.extend(ws.take(20, Duration::from_millis(500)).await);
    let mine = inbox_events(&seen, &item);
    assert_eq!(mine.len(), 1, "{seen:#?}");
    assert_eq!(mine[0].data["status"], "resolved");
    ws.close().await;
}

// ------------------------------------------------------- review round 1
//
// Story 3.4 review round 1 (the coordinator's triage, 2026-11-24): R1 (the
// API runs check 5: approve and return only on the snapshot the run
// pinned, through the `PinCheck` port am-app gives the server) and R2 (a
// resolve whose run cannot go on is refused before anything is written; an
// apply that fails after the resolve committed answers 200 with
// `applied: false`, never 500). Written before the fixes.

fn engine_actor() -> am_domain::Actor {
    am_domain::Actor::System {
        component: "engine".into(),
    }
}

/// The item is open and unresolved, its gate waits, and no
/// `inbox.resolved` entry exists.
fn assert_untouched(rig: &Rig, item: &str) {
    let j = journal(&db(rig));
    let row = j.items.iter().find(|i| i.id == item).expect("the item");
    assert_eq!(row.status, "open", "{row:?}");
    assert_eq!(row.decision, None);
    let gate = j.steps.iter().find(|s| s.id == row.step_run_id).unwrap();
    assert_eq!(gate.status, "waiting_human");
    assert!(j.entries("inbox.resolved").is_empty(), "{:#?}", j.audit);
}

/// A run (pinned to [`PINNED`]) whose step `draft` failed with an open
/// Failure item, by System commands; the item's id.
fn failed_run(rig: &Rig) -> String {
    let store = rig.store.as_ref().unwrap();
    let w = store.writer();
    let run = new_id();
    w.exec(
        engine_actor(),
        WriteCommand::CreateRun {
            run_id: run.clone(),
            workflow: "wf".into(),
            origin: "manual".into(),
            steps: vec!["draft".into(), "approve".into()],
            config_hash: Some(PINNED.into()),
        },
    )
    .unwrap();
    let draft = store.reader().unwrap().steps(&run).unwrap()[0].id.clone();
    w.exec(
        engine_actor(),
        WriteCommand::StartAttempt {
            step_run_id: draft.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
        },
    )
    .unwrap();
    let item = new_id();
    w.exec(
        engine_actor(),
        WriteCommand::FailWithItem {
            step_run_id: draft.clone(),
            settle: None,
            output: None,
            item: NewInboxItem {
                id: item.clone(),
                kind: InboxKind::Failure,
                payload: Some(draft),
                taint: Taint::Clean,
                title: "A step stopped".into(),
                body: "Step `draft` stopped at a cap.".into(),
                max_returns: None,
            },
        },
    )
    .unwrap();
    item
}

/// R1 (Question 2 (a) on every path): while the project's config is not
/// the snapshot the run pinned, an API approve or return is 409
/// `config-changed`, its words naming both hashes, and nothing is written
/// (the item open, the gate waiting). A config error (the pin check's
/// words) fails both closed the same way. Reject and dismiss still work on
/// the changed config. With the config back to the pin, approve goes
/// through.
#[tokio::test]
async fn an_api_resolve_on_a_config_other_than_the_pin_is_refused() {
    let pin = FakePin::new(OTHER);
    let rig = with_pin(Some(pin.clone())).await;
    let run = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
    let item = run.item.clone().unwrap();
    let s = rig.sign_in().await;
    let before = journal(&db(&rig));

    for body in [APPROVE.to_string(), return_with(NOTE)] {
        let r = resolve(&rig, &s, &item, &body).await;
        let e = envelope(&r, 409, "config-changed");
        let words = format!("{} {}", e.message, e.fix.unwrap_or_default());
        for hash in [PINNED, OTHER] {
            assert!(words.contains(&hash[..12]), "names {hash}: {}", r.body);
        }
        unchanged(&before, &rig);
        assert_untouched(&rig, &item);
    }

    pin.set(Err(
        "workflows/gated.yaml:3: a config error the test planted".into(),
    ));
    for body in [APPROVE.to_string(), return_with(NOTE)] {
        let r = resolve(&rig, &s, &item, &body).await;
        envelope(&r, 409, "config-changed");
        unchanged(&before, &rig);
    }
    assert_untouched(&rig, &item);

    // Reject and dismiss need no check.
    pin.set(Ok(OTHER.into()));
    let r = resolve(&rig, &s, &item, REJECT).await;
    assert_eq!(r.status, 200, "reject still works: {}", r.text());
    let failure = failed_run(&rig);
    let r = resolve(&rig, &s, &failure, r#"{"decision":"dismiss"}"#).await;
    assert_eq!(r.status, 200, "dismiss still works: {}", r.text());

    // The config back to the pin: approve goes through.
    pin.set(Ok(PINNED.into()));
    let again = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
    let r = resolve(&rig, &s, again.item.as_deref().unwrap(), APPROVE).await;
    assert_eq!(r.status, 200, "{}", r.text());
}

/// R1: a server given no way to check the config (no `PinCheck`) fails
/// approve and return closed (409 `config-changed`, nothing written);
/// reject still works.
#[tokio::test]
async fn with_no_pin_check_approve_and_return_fail_closed() {
    let rig = with_pin(None).await;
    let run = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
    let item = run.item.clone().unwrap();
    let s = rig.sign_in().await;
    let before = journal(&db(&rig));

    for body in [APPROVE.to_string(), return_with(NOTE)] {
        envelope(
            &resolve(&rig, &s, &item, &body).await,
            409,
            "config-changed",
        );
        unchanged(&before, &rig);
    }
    assert_untouched(&rig, &item);
    let r = resolve(&rig, &s, &item, REJECT).await;
    assert_eq!(r.status, 200, "{}", r.text());
}

/// Why a run cannot go on (R2): what `continue_run` refuses.
#[derive(Debug, Clone, Copy)]
enum Stuck {
    /// Another step of the run is `running`.
    Running,
    /// A model call of the run is reserved and never settled.
    UnsettledCall,
    /// A tool call of the run is `issued` and never finished.
    IssuedToolCall,
}

/// A run (pinned to [`PINNED`]) whose gate `approve` waits with an open
/// Review while the run cannot go on as `how` says, made by System
/// commands; the Review item's id.
fn stuck(rig: &Rig, how: Stuck) -> String {
    let store = rig.store.as_ref().unwrap();
    let w = store.writer();
    let run = new_id();
    w.exec(
        engine_actor(),
        WriteCommand::CreateRun {
            run_id: run.clone(),
            workflow: "wf".into(),
            origin: "manual".into(),
            steps: vec!["draft".into(), "approve".into(), "publish".into()],
            config_hash: Some(PINNED.into()),
        },
    )
    .unwrap();
    let steps = store.reader().unwrap().steps(&run).unwrap();
    let (draft, approve) = (steps[0].id.clone(), steps[1].id.clone());
    w.exec(
        engine_actor(),
        WriteCommand::StartAttempt {
            step_run_id: draft.clone(),
            inputs: vec![],
            taint: Taint::Clean,
            prompt: vec![],
        },
    )
    .unwrap();
    let finish = WriteCommand::FinishStep {
        step_run_id: draft.clone(),
        status: "succeeded".into(),
        output: Some(DRAFT_1.into()),
    };
    let payload = match how {
        Stuck::Running => None,
        Stuck::UnsettledCall => {
            w.exec(
                engine_actor(),
                WriteCommand::ReserveCall {
                    step_run_id: draft.clone(),
                    call_id: new_id(),
                    call_seq: 1,
                    reserved_micro_usd: 0,
                    rate_snapshot: "none".into(),
                    taint: Taint::Clean,
                    purpose: am_store::CallPurpose::Turn,
                    prompt: vec![],
                },
            )
            .unwrap();
            w.exec(engine_actor(), finish).unwrap();
            Some(draft.clone())
        }
        Stuck::IssuedToolCall => {
            w.exec(
                engine_actor(),
                WriteCommand::RecordToolCall {
                    tool_call_id: new_id(),
                    step_run_id: draft.clone(),
                    call_seq: 1,
                    tool: "read_file".into(),
                    status: "issued".into(),
                },
            )
            .unwrap();
            w.exec(engine_actor(), finish).unwrap();
            Some(draft.clone())
        }
    };
    let item = new_id();
    w.exec(
        engine_actor(),
        WriteCommand::PauseStep {
            step_run_id: approve,
            inputs: vec![],
            taint: Taint::Clean,
            item: NewInboxItem {
                id: item.clone(),
                kind: InboxKind::Review,
                payload,
                taint: Taint::Clean,
                title: "Review before step approve goes on".into(),
                body: "Step `approve` waits for your review.".into(),
                max_returns: Some(2),
            },
        },
    )
    .unwrap();
    item
}

/// R2: an API approve or return on a run that `continue_run` would refuse
/// (another step `running`, an unsettled model call, an issued tool call)
/// is refused before anything is written (409 `not-continuable`): the item
/// stays open, the gate waits and no `inbox.resolved` entry is made.
#[tokio::test]
async fn an_api_resolve_whose_run_cannot_go_on_is_refused_before_writing() {
    for how in [Stuck::Running, Stuck::UnsettledCall, Stuck::IssuedToolCall] {
        let rig = with_project_writer().await;
        let item = stuck(&rig, how);
        let s = rig.sign_in().await;
        let before = journal(&db(&rig));

        for body in [APPROVE.to_string(), return_with(NOTE)] {
            let r = resolve(&rig, &s, &item, &body).await;
            assert_eq!(r.status, 409, "{how:?}: {}", r.text());
            envelope(&r, 409, "not-continuable");
            unchanged(&before, &rig);
        }
        assert_untouched(&rig, &item);
    }
}

/// R2: an apply that fails after the person's decision committed (here a
/// trigger planted by raw SQL aborts the gate's finish) answers 200 with
/// the resolved item, the run and `applied: false`, never 500: the
/// decision is recorded, the gate is left `running` for story 3.9's
/// resume.
#[tokio::test]
async fn an_apply_that_fails_after_the_resolve_answers_200_unapplied() {
    let rig = with_project_writer().await;
    let run = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
    let item = run.item.clone().unwrap();
    let s = rig.sign_in().await;
    rusqlite::Connection::open(db(&rig))
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER test_the_apply_fails BEFORE UPDATE OF status ON step_runs
             WHEN OLD.status = 'running' AND NEW.status IN ('succeeded', 'looped_back', 'failed')
             BEGIN SELECT RAISE(ABORT, 'test: the apply cannot commit'); END;",
        )
        .unwrap();

    let r = resolve(&rig, &s, &item, APPROVE).await;

    assert_eq!(
        r.status,
        200,
        "never 500 for a recorded decision: {}",
        r.text()
    );
    let done: ResolutionDto = serde_json::from_str(&r.body).unwrap();
    assert!(!done.applied, "{}", r.body);
    assert_eq!(done.item.id, item);
    assert_eq!(done.item.status, "resolved");
    assert_eq!(done.item.decision.as_deref(), Some("approve"));
    assert_eq!(done.run.id, run.run_id);
    assert_eq!(done.run.status, "running");
    let j = journal(&db(&rig));
    assert_eq!(j.attempt("approve", 1).status, "running");
    assert_eq!(j.entries("inbox.resolved").len(), 1);
}

// ------------------------------------------------------- review round 2
//
// Story 3.4 review round 2 (the coordinator's triage, 2026-11-24), S6: once
// the resolve is recorded, the API answers 200 with `applied: false`
// whatever fails after it, the item's id and `resolved` included when the
// item cannot be read back. Written before the fix.

/// S6: after the person's decision committed, the item cannot be read
/// back. A trigger planted by raw SQL (with the trigger that freezes a
/// resolved row dropped first, as a raw-SQL writer can) rewrites the
/// recorded decision to a spelling no reader knows, in the resolve's own
/// transaction, so the apply and the read-back both fail. The API answers
/// 200, never 500, with `applied: false`, the item's id and `resolved`.
#[tokio::test]
async fn a_resolve_whose_item_cannot_be_read_back_answers_200_unapplied() {
    let rig = with_project_writer().await;
    let run = waiting(&rig, &gated(texts(&[DRAFT_1]))).await;
    let item = run.item.clone().unwrap();
    let s = rig.sign_in().await;
    rusqlite::Connection::open(db(&rig))
        .unwrap()
        .execute_batch(
            "DROP TRIGGER inbox_items_resolved_never_change;
             CREATE TRIGGER test_the_read_back_fails AFTER UPDATE OF status ON inbox_items
             WHEN NEW.status = 'resolved'
             BEGIN UPDATE inbox_items SET decision = 'not-a-decision' WHERE id = NEW.id; END;",
        )
        .unwrap();

    let r = resolve(&rig, &s, &item, APPROVE).await;

    assert_eq!(
        r.status,
        200,
        "never 500 for a recorded decision: {}",
        r.text()
    );
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(v["applied"], false, "{}", r.body);
    assert_eq!(v["item"]["id"], item.as_str(), "{}", r.body);
    assert_eq!(v["item"]["status"], "resolved", "{}", r.body);
    let status: String = rusqlite::Connection::open(db(&rig))
        .unwrap()
        .query_row(
            "SELECT status FROM inbox_items WHERE id = ?1",
            [&item],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, "resolved", "the decision is recorded");
}
