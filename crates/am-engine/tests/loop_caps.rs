//! Epic 3 story 3: the bounded agent loop (AD-8, AD-11, RULE-11; matrix
//! rows 15 to 18, 25 and 28). A step attempt is a loop of model turns; a
//! turn that asks for tools goes on once ToolGate has decided each call and
//! each allowed call's result is appended. Three caps per attempt
//! (`StepPlan::limits`): iterations (model turns), tool calls and
//! wall-clock. A cap that is hit ends the attempt `failed` with words that
//! name the cap and its value, settles the in-flight call by the ledger
//! rule, fails the run and raises one Failure Inbox item. Every count comes
//! from journal rows (`model_calls`, `tool_calls`, the attempt's persisted
//! `started_at`), never from memory.
//!
//! The shipped binary has no tool: `run_plan` takes none, and a tool
//! request fails the step in story 3.1's words. Tools exist only through
//! `run_plan_with_tools`, a constructor parameter shipped code never passes
//! (owner condition 2; `cargo xtask seam-guard`), here with am-testkit's
//! `FixtureTool`. No network, no real model; the wall-clock cap runs on
//! tokio's paused time. Written before the loop exists.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_domain::tool_manifest::{
    ActionClass as ManifestClass, NetworkNeed, ToolManifest as DomainManifest,
};
use am_domain::{
    Actor, BoxFuture, CancelSignal, ChatOutcome, ChatRequest, EventSink, InboxKind, Instruction,
    LlmProvider, ModelCapabilities, ProviderError, ToolCall, ToolRunner, UntrustedData,
};
use am_engine::run::{
    HandOff, Limits, RunOutcome, RunPlan, StepPlan, run_plan, run_plan_with_tools,
};
use am_store::{CallRow, InboxItemRow, LedgerRow, Reader, StepRow, Store, ToolCallRow};
use am_testkit::{FakeLlm, FixtureTool, ScriptedTurn};
use tokio_util::sync::CancellationToken;

const HASH: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

struct Project {
    _dir: tempfile::TempDir,
    db: PathBuf,
    store: Store,
}

fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("state.db");
    let store = Store::open(&db).unwrap();
    Project {
        _dir: dir,
        db,
        store,
    }
}

fn step(provider: Arc<dyn LlmProvider>, limits: Limits) -> StepPlan {
    StepPlan {
        key: "work".into(),
        seat: "seat-work".into(),
        system: vec![Instruction::authored("You are the seat of step work.")],
        instruction: Instruction::authored("Do the work."),
        inputs: vec![],
        hand_off: HandOff::Next,
        limits,
        model: "example-local".into(),
        max_output_tokens: 256,
        provider,
    }
}

fn plan(steps: Vec<StepPlan>) -> RunPlan {
    RunPlan {
        workflow: "wf".into(),
        config_hash: HASH.into(),
        steps,
    }
}

fn limits(iterations: u32, tool_calls: u32, wall_clock: Duration) -> Limits {
    Limits {
        iterations,
        tool_calls,
        wall_clock,
    }
}

/// A turn that asks for the fixture tool `n` times.
fn asks_for_tools(n: usize) -> ScriptedTurn {
    let mut turn = ScriptedTurn::text("let me look");
    for i in 0..n {
        turn = turn.tool_call(FixtureTool::NAME, &format!("{{\"n\":{i}}}"));
    }
    turn
}

async fn with_tools(p: &Project, plan: &RunPlan, tool: &Arc<FixtureTool>) -> RunOutcome {
    run_plan_with_tools(
        &p.store,
        owner(),
        "manual",
        plan,
        CancellationToken::new(),
        tool.clone(),
    )
    .await
    .unwrap()
}

/// The one step attempt's rows, through a fresh read-only connection.
struct Rows {
    step: StepRow,
    calls: Vec<CallRow>,
    ledger: Vec<LedgerRow>,
    tools: Vec<ToolCallRow>,
    items: Vec<InboxItemRow>,
    run_status: String,
}

fn rows(db: &Path) -> Rows {
    let reader = Reader::open(db).unwrap();
    let run = reader.runs().unwrap().remove(0);
    let mut steps = reader.steps(&run.id).unwrap();
    assert_eq!(steps.len(), 1, "{steps:#?}");
    let step = steps.remove(0);
    Rows {
        tools: reader.tool_calls(&step.id).unwrap(),
        calls: reader.calls_for_run(&run.id).unwrap(),
        ledger: reader.ledger_for_run(&run.id).unwrap(),
        items: reader.inbox_items(&run.id).unwrap(),
        run_status: run.status,
        step,
    }
}

/// True when `text` names `n` as a word of its own (never a UUID's digit).
fn names_number(text: &str, n: u64) -> bool {
    let n = n.to_string();
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == n || w.strip_suffix(['s', 'm', 'h']).is_some_and(|d| d == n))
}

/// One open Failure item on the attempt, in engine words naming `cap` and
/// `value`.
fn assert_failure_item(r: &Rows, cap: &str, value: u64) {
    assert_eq!(r.items.len(), 1, "{:#?}", r.items);
    let item = &r.items[0];
    assert_eq!(item.kind, InboxKind::Failure);
    assert_eq!(item.status, "open");
    assert_eq!(item.step_run_id, r.step.id);
    assert!(item.body.contains(cap), "names the cap: {}", item.body);
    assert!(
        names_number(&item.body, value),
        "names {value}: {}",
        item.body
    );
    assert!(
        !item.body.contains("let me look"),
        "no model text: {}",
        item.body
    );
}

/// Rows 15 and 18: `limits.iterations: 3` and a model that asks for the
/// fixture tool every turn. The third turn's tool request ends the attempt
/// `failed`, naming `iterations` and 3: three model calls, all settled; the
/// run fails; one Failure item. The count acted on is the journal's.
#[tokio::test]
async fn the_iteration_cap_ends_the_step_with_a_failure_item() {
    let p = project();
    let tool = FixtureTool::new();
    let llm = Arc::new(FakeLlm::new(vec![asks_for_tools(1)]));
    let plan = plan(vec![step(
        llm.clone(),
        limits(3, 20, Duration::from_secs(600)),
    )]);

    let outcome = with_tools(&p, &plan, &tool).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let reason = outcome.reason.clone().expect("in words");
    assert!(
        reason.contains("iterations") && names_number(&reason, 3),
        "{reason}"
    );
    assert_eq!(llm.calls(), 3, "never a fourth turn");
    let r = rows(&p.db);
    assert_eq!(r.step.status, "failed");
    assert_eq!(r.calls.len(), 3, "three model_calls rows");
    assert!(r.calls.iter().all(|c| c.settled), "{:#?}", r.calls);
    assert!(r.ledger.iter().all(|l| l.state == "settled"));
    assert!(r.tools.len() <= 3, "{:#?}", r.tools);
    assert!(tool.calls() >= 2, "the first two turns' tools ran");
    assert_eq!(r.run_status, "failed");
    assert_failure_item(&r, "iterations", 3);
}

/// Rows 16 and 18: `limits.tool_calls: 2` and one turn that asks for three
/// tool calls. Calls 1 and 2 run; the third ends the attempt `failed`,
/// naming `tool_calls` and 2: three `tool_calls` rows, the third
/// `refused`; one model call, settled; one Failure item.
#[tokio::test]
async fn the_tool_call_cap_ends_the_step_with_a_failure_item() {
    let p = project();
    let tool = FixtureTool::new();
    let llm = Arc::new(FakeLlm::new(vec![
        asks_for_tools(3),
        ScriptedTurn::text("never reached"),
    ]));
    let plan = plan(vec![step(
        llm.clone(),
        limits(8, 2, Duration::from_secs(600)),
    )]);

    let outcome = with_tools(&p, &plan, &tool).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let reason = outcome.reason.clone().expect("in words");
    assert!(
        reason.contains("tool_calls") && names_number(&reason, 2),
        "{reason}"
    );
    assert_eq!(tool.calls(), 2, "calls 1 and 2 ran, the third did not");
    assert_eq!(llm.calls(), 1);
    let r = rows(&p.db);
    assert_eq!(r.step.status, "failed");
    assert_eq!(r.tools.len(), 3, "{:#?}", r.tools);
    assert!(r.tools.iter().all(|t| t.tool == FixtureTool::NAME));
    assert!(
        r.tools.windows(2).all(|w| w[0].call_seq < w[1].call_seq),
        "{:#?}",
        r.tools
    );
    let statuses: Vec<&str> = r.tools.iter().map(|t| t.status.as_str()).collect();
    assert_ne!(statuses[0], "refused");
    assert_ne!(statuses[1], "refused");
    assert_eq!(statuses[2], "refused");
    assert_eq!(r.calls.len(), 1);
    assert!(r.calls[0].settled);
    assert_eq!(r.run_status, "failed");
    assert_failure_item(&r, "tool_calls", 2);
}

/// Row 17: `limits.wall_clock: 30s` and a model that never answers, on
/// tokio's paused time. At 30 s the in-flight call is cancelled through its
/// own child token (the run's token is untouched) and settled at its
/// reservation; the attempt fails naming `wall_clock`; one Failure item.
#[tokio::test(start_paused = true)]
async fn the_wall_clock_cap_cancels_and_settles_the_call() {
    let p = project();
    let tool = FixtureTool::new();
    let llm = Arc::new(FakeLlm::new(vec![ScriptedTurn::hang()]));
    let plan = plan(vec![step(
        llm.clone(),
        limits(8, 20, Duration::from_secs(30)),
    )]);
    let run_token = CancellationToken::new();
    let started = tokio::time::Instant::now();

    let outcome = run_plan_with_tools(
        &p.store,
        owner(),
        "manual",
        &plan,
        run_token.clone(),
        tool.clone(),
    )
    .await
    .unwrap();

    let waited = started.elapsed();
    assert!(
        waited >= Duration::from_secs(30) && waited < Duration::from_secs(60),
        "{waited:?}"
    );
    assert!(
        !run_token.is_cancelled(),
        "only the call's token is cancelled"
    );
    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let reason = outcome.reason.clone().expect("in words");
    assert!(
        reason.contains("wall_clock") && names_number(&reason, 30),
        "{reason}"
    );
    assert_eq!(llm.calls(), 1);
    let r = rows(&p.db);
    assert_eq!(r.step.status, "failed");
    assert_eq!(r.calls.len(), 1);
    assert!(r.calls[0].settled, "the cancelled call is settled");
    assert_eq!(
        (r.calls[0].input_tokens, r.calls[0].output_tokens),
        (None, None),
        "no usage: at its reservation"
    );
    assert_eq!(r.ledger[0].state, "settled");
    assert_eq!(
        r.ledger[0].settled_micro_usd,
        Some(r.ledger[0].reserved_micro_usd)
    );
    assert_eq!(r.run_status, "failed");
    assert_failure_item(&r, "wall_clock", 30);
}

/// A provider that reads the journal when it is called, then plays its
/// `FakeLlm`'s next turn.
struct Spy {
    db: PathBuf,
    llm: Arc<FakeLlm>,
    seen: Mutex<Vec<(usize, usize, Option<String>)>>,
}

impl LlmProvider for Spy {
    fn account_id(&self) -> String {
        "spy".into()
    }
    fn capabilities(&self, model: &str) -> ModelCapabilities {
        self.llm.capabilities(model)
    }
    fn check_file_parts(&self, request: &ChatRequest) -> Result<(), ProviderError> {
        self.llm.check_file_parts(request)
    }
    fn stream(
        &self,
        request: ChatRequest,
        cancel: Arc<dyn CancelSignal>,
        sink: Arc<dyn EventSink>,
    ) -> BoxFuture<Result<ChatOutcome, ProviderError>> {
        let reader = Reader::open(&self.db).unwrap();
        let run = reader.runs().unwrap().remove(0);
        let step = reader.steps(&run.id).unwrap().remove(0);
        let calls = reader.calls_for_run(&run.id).unwrap().len();
        let tools = reader.tool_calls(&step.id).unwrap().len();
        self.seen
            .lock()
            .unwrap()
            .push((calls, tools, step.started_at.clone()));
        self.llm.stream(request, cancel, sink)
    }
}

/// Row 28: the process stops during a turn (the run's future is dropped
/// while the second model call is in flight). The rows stay as committed:
/// the attempt `running` with its persisted start, the first call settled,
/// the second reserved and open, one tool call; nothing resumes (story
/// 3.9). The counts the engine acted on before each call (model calls,
/// tool calls, the start time) are the ones a fresh reader derives.
#[tokio::test]
async fn counts_a_fresh_reader_derives_match_the_engines() {
    let p = project();
    let tool = FixtureTool::new();
    let spy = Arc::new(Spy {
        db: p.db.clone(),
        llm: Arc::new(FakeLlm::new(vec![asks_for_tools(1), ScriptedTurn::hang()])),
        seen: Mutex::new(Vec::new()),
    });
    let plan = plan(vec![step(
        spy.clone(),
        limits(8, 20, Duration::from_secs(600)),
    )]);

    let in_flight = async {
        loop {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if spy.seen.lock().unwrap().len() == 2 {
                // Let the hanging call settle into its await.
                tokio::time::sleep(Duration::from_millis(20)).await;
                return;
            }
        }
    };
    tokio::select! {
        done = run_plan_with_tools(
            &p.store,
            owner(),
            "manual",
            &plan,
            CancellationToken::new(),
            tool.clone(),
        ) => panic!("the run must still be in its second turn: {done:?}"),
        () = in_flight => {}
    }

    // What the engine had committed before each call.
    let seen = spy.seen.lock().unwrap().clone();
    assert_eq!(seen[0].0, 1, "call 1's reservation before call 1");
    assert_eq!(seen[0].1, 0);
    assert_eq!(seen[1].0, 2, "call 2's reservation before call 2");
    assert_eq!(seen[1].1, 1, "the tool call before call 2");
    let started = seen[0].2.clone().expect("the attempt's start is persisted");
    assert_eq!(seen[1].2.as_deref(), Some(started.as_str()));

    // A fresh reader, after the stop.
    let r = rows(&p.db);
    assert_eq!(r.step.status, "running");
    assert_eq!(r.step.started_at.as_deref(), Some(started.as_str()));
    assert_eq!(r.step.finished_at, None);
    assert_eq!(r.calls.len(), 2);
    assert!(r.calls[0].settled && !r.calls[1].settled, "{:#?}", r.calls);
    assert_eq!(
        r.ledger
            .iter()
            .map(|l| l.state.as_str())
            .collect::<Vec<_>>(),
        vec!["settled", "reserved"]
    );
    assert_eq!(r.tools.len(), 1);
    assert_eq!(tool.calls(), 1);
    assert_eq!(r.run_status, "running");
    assert!(r.items.is_empty());
    // Reopening the store resumes nothing in this story.
    let Project { _dir, db, store } = p;
    drop(store);
    let reopened = Store::open(&db).unwrap();
    let again = rows(&db);
    assert_eq!(again.step, r.step);
    assert_eq!(again.calls, r.calls);
    assert_eq!(again.tools, r.tools);
    drop(reopened);
}

/// Row 25: with no tools (`run_plan`, as the binary runs), a tool request
/// fails the step in story 3.1's words, with no `tool_calls` row and no
/// Failure item (no cap was hit).
#[tokio::test]
async fn with_no_tools_a_tool_request_fails_as_before() {
    let p = project();
    for name in [FixtureTool::NAME, "web_search"] {
        let llm = Arc::new(FakeLlm::new(vec![
            ScriptedTurn::text("let me look").tool_call(name, "{}"),
        ]));
        let plan = plan(vec![step(llm.clone(), Limits::default())]);

        let outcome = run_plan(&p.store, owner(), "manual", &plan, CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(outcome.status, "failed", "{name}");
        let reason = outcome.reason.clone().expect("in words");
        assert!(
            reason.contains("tools are not available in this version"),
            "{name}: {reason}"
        );
        assert!(
            llm.requests()[0].tools.is_empty(),
            "{name}: no tool offered"
        );
        let reader = Reader::open(&p.db).unwrap();
        let run = reader.run(&outcome.run_id).unwrap().unwrap();
        let steps = reader.steps(&run.id).unwrap();
        assert!(
            reader.tool_calls(&steps[0].id).unwrap().is_empty(),
            "{name}"
        );
        assert!(reader.inbox_items(&run.id).unwrap().is_empty(), "{name}");
    }
}

/// The defaults of `Limits` (Question 7, placeholders the owner accepted):
/// iterations 8, tool calls 20, wall-clock 10 minutes.
#[test]
fn the_default_caps_are_the_placeholders() {
    assert_eq!(
        Limits::default(),
        limits(8, 20, Duration::from_secs(10 * 60))
    );
}

// ------------------------------------------------------- review round 1
//
// Story 3.3 review patch round 1 (coordinator's triage, 2026-11-23): P1
// (a fresh attempt's whole wall-clock cap), P2 (ToolGate refuses every
// class but `Read` and is the only caller of the runner), P7 (nothing
// starts after the deadline; a tie goes to the deadline), P8 (ToolGate's
// per-step uses are per attempt), P13 (a seat's run id is the run's, never
// a nil UUID) and P15 (a model-chosen tool name is never journaled,
// audited or reported raw). Written before the patches.

/// [`step`] under another key.
fn keyed(key: &str, provider: Arc<dyn LlmProvider>, limits: Limits) -> StepPlan {
    StepPlan {
        key: key.into(),
        seat: format!("seat-{key}"),
        system: vec![Instruction::authored(format!(
            "You are the seat of step {key}."
        ))],
        instruction: Instruction::authored(format!("Do the work of step {key}.")),
        ..step(provider, limits)
    }
}

/// One tool of [`Tools`]: its id, class, per-step maximum and how long it
/// takes (on tokio's clock; cancel ends it).
#[derive(Clone)]
struct Tool {
    id: &'static str,
    class: ManifestClass,
    max_uses_per_step: u32,
    takes: Duration,
}

impl Tool {
    fn read(id: &'static str) -> Tool {
        Tool {
            id,
            class: ManifestClass::Read,
            max_uses_per_step: 100,
            takes: Duration::ZERO,
        }
    }
}

/// A test-local `ToolRunner` (tests are not shipped): its tools, and the
/// id of every call it ran, in order.
struct Tools {
    tools: Vec<Tool>,
    ran: Mutex<Vec<String>>,
}

impl Tools {
    fn new(tools: Vec<Tool>) -> Arc<Tools> {
        Arc::new(Tools {
            tools,
            ran: Mutex::new(Vec::new()),
        })
    }

    fn ran(&self) -> Vec<String> {
        self.ran.lock().unwrap().clone()
    }
}

impl ToolRunner for Tools {
    fn manifests(&self) -> Vec<DomainManifest> {
        self.tools
            .iter()
            .map(|t| DomainManifest {
                id: t.id.to_string(),
                class: t.class,
                network: NetworkNeed::None,
                max_uses_per_step: t.max_uses_per_step,
            })
            .collect()
    }

    fn run(
        &self,
        call: ToolCall,
        cancel: Arc<dyn CancelSignal>,
    ) -> BoxFuture<Result<UntrustedData, String>> {
        self.ran.lock().unwrap().push(call.name.clone());
        let takes = self
            .tools
            .iter()
            .find(|t| t.id == call.name)
            .map(|t| t.takes)
            .unwrap_or_default();
        Box::pin(async move {
            tokio::select! {
                () = cancel.cancelled() => Err("cancelled".to_string()),
                () = tokio::time::sleep(takes) => Ok(UntrustedData::text(
                    format!("tool:{}", call.name),
                    format!("the result of {}", call.name),
                )),
            }
        })
    }
}

/// The step-run ids of `key`'s attempts in the only run, in order.
fn attempts_of(db: &Path, key: &str) -> Vec<StepRow> {
    let reader = Reader::open(db).unwrap();
    let run = reader.runs().unwrap().remove(0);
    reader
        .steps(&run.id)
        .unwrap()
        .into_iter()
        .filter(|s| s.step_key == key)
        .collect()
}

/// A provider that, when it is called, sets the persisted `started_at` of
/// the still-queued step `next` to `at(now)` (an instant before now, as a
/// clock that jumped, or a start that fell just before a whole second,
/// leaves it), then answers `reply`.
struct BackdatesNext {
    db: PathBuf,
    next: &'static str,
    at: fn(jiff::Timestamp) -> jiff::Timestamp,
    reply: &'static str,
}

impl LlmProvider for BackdatesNext {
    fn account_id(&self) -> String {
        "backdates-next".into()
    }
    fn capabilities(&self, _model: &str) -> ModelCapabilities {
        ModelCapabilities::default()
    }
    fn check_file_parts(&self, _request: &ChatRequest) -> Result<(), ProviderError> {
        Ok(())
    }
    fn stream(
        &self,
        _request: ChatRequest,
        _cancel: Arc<dyn CancelSignal>,
        _sink: Arc<dyn EventSink>,
    ) -> BoxFuture<Result<ChatOutcome, ProviderError>> {
        let earlier = (self.at)(jiff::Timestamp::now())
            .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();
        let changed = backdate(&self.db, self.next, &earlier);
        assert_eq!(changed, 1, "one queued attempt of {}", self.next);
        let reply = ChatOutcome {
            text: self.reply.into(),
            tool_calls: vec![],
            usage: None,
            stop: am_domain::StopReason::EndTurn,
        };
        Box::pin(async move { Ok(reply) })
    }
}

/// Sets `started_at` of the queued attempt of `key` to `at`, raw (the
/// writer has no command that does it); the number of rows changed.
fn backdate(db: &Path, key: &str, at: &str) -> usize {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.execute(
        "UPDATE step_runs SET started_at = ?2 WHERE step_key = ?1 AND status = 'queued'",
        rusqlite::params![key, at],
    )
    .unwrap()
}

/// P1: a fresh attempt waits its whole wall-clock cap, whatever its
/// persisted `started_at` says about real time: the deadline of an attempt
/// the engine just started comes from tokio's clock right after
/// `StartAttempt` (the elapsed real time counts only for a resumed
/// attempt). Here the step before it moves the waiting step's
/// `started_at` 10 s back before the step starts (`StartAttempt` keeps a
/// `started_at` already there); the step still gets its 30 s.
#[tokio::test(start_paused = true)]
async fn a_fresh_attempt_gets_its_whole_wall_clock_cap() {
    let p = project();
    let first = Arc::new(BackdatesNext {
        db: p.db.clone(),
        next: "work",
        at: |now| now - jiff::SignedDuration::from_secs(10),
        reply: "ready",
    });
    let hang = Arc::new(FakeLlm::new(vec![ScriptedTurn::hang()]));
    let plan = plan(vec![
        keyed("prep", first, Limits::default()),
        keyed("work", hang, limits(8, 20, Duration::from_secs(30))),
    ]);
    let started = tokio::time::Instant::now();

    let outcome = run_plan(&p.store, owner(), "manual", &plan, CancellationToken::new())
        .await
        .unwrap();

    let waited = started.elapsed();
    assert_eq!(outcome.status, "failed", "{outcome:?}");
    assert!(
        outcome
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("wall_clock"),
        "{outcome:?}"
    );
    assert!(
        waited >= Duration::from_secs(30),
        "a fresh attempt waits its whole cap of 30 s, not {waited:?}"
    );
}

/// P1, as the flake was seen: a fresh attempt whose `started_at` falls
/// 1 ms before a whole second of real time (so that "now" is in the next
/// second however fast the engine is) still waits its whole 30 s cap; a
/// deadline built from whole seconds lost one.
#[tokio::test(start_paused = true)]
async fn a_fresh_attempt_started_just_before_a_second_boundary_waits_the_full_cap() {
    let p = project();
    let first = Arc::new(BackdatesNext {
        db: p.db.clone(),
        next: "work",
        at: |now| {
            jiff::Timestamp::from_second(now.as_second()).unwrap()
                - jiff::SignedDuration::from_millis(1)
        },
        reply: "ready",
    });
    let hang = Arc::new(FakeLlm::new(vec![ScriptedTurn::hang()]));
    let plan = plan(vec![
        keyed("prep", first, Limits::default()),
        keyed("work", hang, limits(8, 20, Duration::from_secs(30))),
    ]);
    let started = tokio::time::Instant::now();

    let outcome = run_plan(&p.store, owner(), "manual", &plan, CancellationToken::new())
        .await
        .unwrap();

    let waited = started.elapsed();
    assert_eq!(outcome.status, "failed", "{outcome:?}");
    assert!(
        waited >= Duration::from_secs(30),
        "a start at x.999 s still gets the whole 30 s cap, not {waited:?}"
    );
}

/// P2: ToolGate refuses every action class but `Read` on this path (a
/// local-write or external-effect tool waits for stories 3.6 and 4.8): the
/// call is refused and journaled `refused`, the step fails in words, and
/// the runner is never asked to run it.
#[tokio::test]
async fn a_tool_whose_class_is_not_read_is_refused_and_never_runs() {
    for class in [ManifestClass::LocalWrite, ManifestClass::ExternalEffect] {
        let p = project();
        let tools = Tools::new(vec![Tool {
            class,
            ..Tool::read("act_on_it")
        }]);
        let llm = Arc::new(FakeLlm::new(vec![
            ScriptedTurn::text("doing it").tool_call("act_on_it", "{}"),
            ScriptedTurn::text("never reached"),
        ]));
        let plan = plan(vec![step(llm.clone(), Limits::default())]);

        let outcome = run_plan_with_tools(
            &p.store,
            owner(),
            "manual",
            &plan,
            CancellationToken::new(),
            tools.clone(),
        )
        .await
        .unwrap();

        assert!(tools.ran().is_empty(), "{class:?}: the runner never ran it");
        assert_eq!(outcome.status, "failed", "{class:?}: {outcome:?}");
        assert!(outcome.reason.is_some(), "{class:?}: in words");
        assert_eq!(llm.calls(), 1, "{class:?}");
        let r = rows(&p.db);
        assert_eq!(r.tools.len(), 1, "{class:?}: {:#?}", r.tools);
        assert_eq!(r.tools[0].status, "refused", "{class:?}");
    }
}

/// The code of a Rust source with its `//` comments removed and all
/// whitespace dropped (a heuristic view for the structural checks below).
fn flat_code(src: &str) -> String {
    src.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<String>()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// Every `.rs` file under am-engine's `src/`, as (name, text).
fn engine_sources() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                out.push((name, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    out
}

/// P2, structural: ToolGate checks, then runs (AD-11's single choke
/// point): outside `toolgate.rs`, am-engine's source never runs a
/// `ToolRunner` itself (no `runner.run(`, no `.runner.` field reach, no
/// `ToolRunner::run(`).
#[test]
fn only_toolgate_runs_a_tool() {
    let sources = engine_sources();
    assert!(sources.iter().any(|(n, _)| n == "toolgate.rs"));
    for (name, text) in sources.iter().filter(|(n, _)| n != "toolgate.rs") {
        let code = flat_code(text);
        for pattern in ["runner.run(", ".runner.", "ToolRunner::run("] {
            assert!(
                !code.contains(pattern),
                "{name} runs a tool past ToolGate (`{pattern}`); only ToolGate calls ToolRunner::run"
            );
        }
    }
}

/// P7: a model reply ready at the same instant as the wall-clock deadline
/// loses to the deadline: the attempt fails at its cap and nothing the
/// reply asked for runs (no tool call is journaled or run, no second
/// call). Repeated, since an unbiased choice would pick either half the
/// time.
#[tokio::test(start_paused = true)]
async fn a_reply_at_the_deadline_loses_to_the_deadline() {
    for trial in 0..16 {
        let p = project();
        let tool = FixtureTool::new();
        let llm = Arc::new(FakeLlm::new(vec![
            asks_for_tools(1).delay(Duration::from_secs(30)),
            ScriptedTurn::text("never reached"),
        ]));
        let plan = plan(vec![step(llm, limits(8, 20, Duration::from_secs(30)))]);

        let outcome = with_tools(&p, &plan, &tool).await;

        assert_eq!(outcome.status, "failed", "trial {trial}: {outcome:?}");
        assert!(
            outcome
                .reason
                .as_deref()
                .unwrap_or_default()
                .contains("wall_clock"),
            "trial {trial}: {outcome:?}"
        );
        let r = rows(&p.db);
        assert!(r.tools.is_empty(), "trial {trial}: {:#?}", r.tools);
        assert_eq!(tool.calls(), 0, "trial {trial}");
        assert_eq!(r.calls.len(), 1, "trial {trial}: no second call");
    }
}

/// P7: once the deadline has passed, no new tool call is recorded and no
/// new model call is reserved. The first tool of the turn ends exactly at
/// the deadline; the second tool, and the next turn, never start.
/// Repeated, as above.
#[tokio::test(start_paused = true)]
async fn nothing_starts_after_the_deadline() {
    for trial in 0..16 {
        let p = project();
        let tools = Tools::new(vec![
            Tool {
                takes: Duration::from_secs(30),
                ..Tool::read("slow_read")
            },
            Tool::read("fast_read"),
        ]);
        let llm = Arc::new(FakeLlm::new(vec![
            ScriptedTurn::text("let me look")
                .tool_call("slow_read", "{}")
                .tool_call("fast_read", "{}"),
            ScriptedTurn::text("never reached"),
        ]));
        let plan = plan(vec![step(llm, limits(8, 20, Duration::from_secs(30)))]);

        let outcome = run_plan_with_tools(
            &p.store,
            owner(),
            "manual",
            &plan,
            CancellationToken::new(),
            tools.clone(),
        )
        .await
        .unwrap();

        assert_eq!(outcome.status, "failed", "trial {trial}: {outcome:?}");
        let r = rows(&p.db);
        assert_eq!(r.calls.len(), 1, "trial {trial}: no call reserved after it");
        assert!(
            r.tools.iter().all(|t| t.tool != "fast_read"),
            "trial {trial}: no tool call recorded after it: {:#?}",
            r.tools
        );
        assert!(
            !tools.ran().iter().any(|t| t == "fast_read"),
            "trial {trial}: {:?}",
            tools.ran()
        );
    }
}

/// P8: ToolGate's per-step uses count per attempt: a tool allowed once per
/// step runs once in each attempt, so a step re-run after a loop-back may
/// use it again.
#[tokio::test]
async fn a_new_attempt_starts_its_tool_uses_at_zero() {
    let p = project();
    let tools = Tools::new(vec![Tool {
        max_uses_per_step: 1,
        ..Tool::read("once_read")
    }]);
    let work = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("looking").tool_call("once_read", "{}"),
        ScriptedTurn::text("version one"),
        ScriptedTurn::text("looking again").tool_call("once_read", "{}"),
        ScriptedTurn::text("version two"),
    ]));
    let check = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text(r#"{"reasons":["look again"]}"#),
        ScriptedTurn::text(r#"{"reasons":[]}"#),
    ]));
    let checking = StepPlan {
        hand_off: HandOff::LoopBack {
            to: "work".into(),
            field: "reasons".into(),
            max: 2,
        },
        ..keyed("check", check, Limits::default())
    };
    let plan = plan(vec![keyed("work", work, Limits::default()), checking]);

    let outcome = run_plan_with_tools(
        &p.store,
        owner(),
        "manual",
        &plan,
        CancellationToken::new(),
        tools.clone(),
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "succeeded", "{outcome:?}");
    assert_eq!(tools.ran(), vec!["once_read", "once_read"]);
    let attempts = attempts_of(&p.db, "work");
    assert_eq!(attempts.len(), 2);
    assert!(
        attempts.iter().all(|s| s.status == "succeeded"),
        "{attempts:#?}"
    );
}

/// The audit export, one JSON value per entry.
fn audit_of(db: &Path) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    Reader::open(db)
        .unwrap()
        .export_audit_jsonl(&mut out)
        .unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// P13, behaviour: a tool call acts as `Actor::Seat` of this very run (its
/// id parsed from the run's id, never a nil UUID): ToolGate's refusal of an
/// unknown tool is audited as the step's seat in this run.
#[tokio::test]
async fn a_tool_call_acts_as_the_seat_of_this_run() {
    let p = project();
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("").tool_call("no_such_tool", "{}"),
    ]));
    let plan = plan(vec![step(llm, Limits::default())]);

    let outcome = with_tools(&p, &plan, &FixtureTool::new()).await;

    assert_eq!(outcome.status, "failed", "{outcome:?}");
    let refusals: Vec<serde_json::Value> = audit_of(&p.db)
        .into_iter()
        .filter(|e| e["action"] == "toolgate.refused")
        .collect();
    assert_eq!(refusals.len(), 1, "{refusals:#?}");
    let actor = &refusals[0]["actor"];
    assert_eq!(actor["kind"], "seat", "{actor}");
    assert_eq!(actor["seat"], "seat-work");
    assert_eq!(actor["step"], "work");
    assert_eq!(actor["run"], outcome.run_id.as_str());
    assert_ne!(actor["run"], uuid::Uuid::nil().to_string().as_str());
}

/// P13, structural: am-engine never turns an id it cannot parse into a
/// default (nil) UUID; an unparsable run id is an error in words.
#[test]
fn an_unparsable_run_id_is_never_a_nil_uuid() {
    for (name, text) in engine_sources() {
        let code = flat_code(&text);
        assert!(!code.contains("Uuid::nil("), "{name}");
        let mut from = 0;
        while let Some(at) = code[from..].find("parse_str(") {
            let start = from + at;
            let rest = &code[start..];
            let statement = &rest[..rest.find(';').unwrap_or(rest.len())];
            assert!(
                !statement.contains("unwrap_or_default()") && !statement.contains("unwrap_or("),
                "{name}: an id that does not parse becomes a made-up UUID: {statement}"
            );
            from = start + "parse_str(".len();
        }
    }
}

/// The sha256 of `text`'s UTF-8 bytes, lowercase hex.
fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// No character below a space (no escape, bell or newline).
fn printable(text: &str) -> bool {
    !text.chars().any(|c| c.is_control())
}

/// P15: a tool name the model chose and no manifest holds (a plain unknown
/// name, one with control characters, one 10 KB long) is kept in
/// `tool_calls` and the audit log as `unknown` plus the sha256 of the name,
/// never raw; the run's words carry no control character and no part of
/// the raw name.
#[tokio::test]
async fn an_unknown_tool_name_is_kept_as_unknown_and_its_hash() {
    let long = "x".repeat(10 * 1024);
    let cases: [(&str, String, String); 3] = [
        (
            "unknown",
            "exfiltrate_every_key".to_string(),
            "exfiltrate_every_key".to_string(),
        ),
        (
            "control characters",
            "zq9\u{1b}[2J\u{7}wipe\nname".to_string(),
            "zq9".to_string(),
        ),
        ("10 KB", long.clone(), "x".repeat(64)),
    ];
    for (what, name, marker) in cases {
        let p = project();
        let llm = Arc::new(FakeLlm::new(vec![
            ScriptedTurn::text("").tool_call(&name, "{}"),
        ]));
        let plan = plan(vec![step(llm, Limits::default())]);

        let outcome = with_tools(&p, &plan, &FixtureTool::new()).await;

        assert_eq!(outcome.status, "failed", "{what}: {outcome:?}");
        let hash = sha256_hex(&name);
        let r = rows(&p.db);
        assert_eq!(r.tools.len(), 1, "{what}: {:#?}", r.tools.len());
        let kept = &r.tools[0].tool;
        assert!(kept.starts_with("unknown"), "{what}: {kept:.200}");
        assert!(
            kept.contains(&hash),
            "{what}: the name's sha256: {kept:.200}"
        );
        assert!(!kept.contains(&marker), "{what}: never raw: {kept:.200}");
        assert!(printable(kept) && kept.len() <= 256, "{what}: {kept:.200}");
        let audit = audit_of(&p.db)
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !audit.contains(&marker),
            "{what}: the audit log holds the raw name"
        );
        assert!(
            !audit.contains("\\u001b") && !audit.contains("\\u0007"),
            "{what}: control characters in the audit log"
        );
        assert!(
            audit.contains(&hash),
            "{what}: the audit log names the hash"
        );
        let reason = outcome.reason.clone().unwrap_or_default();
        assert!(
            printable(&reason) && !reason.contains(&marker) && reason.len() < 2000,
            "{what}: the run's words: {reason:.300}"
        );
    }
}
