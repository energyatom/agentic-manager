//! Story 3.1 (epic 3 story 1): the engine's one-turn-per-step run driver
//! (AD-8, AD-6, AD-16), on a real store with am-testkit's `FakeLlm` and two
//! small providers of this file (one that reads the journal when it is
//! called, one that returns a fixed outcome). No network, no real model.
//! Written before `am_engine::run` exists.
//!
//! What these tests pin (plain types, no config: am-app maps a config
//! snapshot to them, AD-4):
//!
//! ```text
//! am_engine::run::RunPlan  { workflow: String, config_hash: String, steps: Vec<StepPlan> }
//! am_engine::run::StepPlan { key: String, system: Vec<Instruction>, instruction: Instruction,
//!                            model: String, max_output_tokens: u32,
//!                            provider: Arc<dyn LlmProvider> }
//!     (story 3.3 adds seat: String, inputs: Vec<UntrustedData>,
//!      hand_off: HandOff, limits: Limits; see tests/loop_caps.rs)
//! am_engine::run::run_plan(store: &Store, started_by: Actor, origin: &str, plan: &RunPlan,
//!                          cancel: CancellationToken) -> Result<outcome, error>
//!     outcome { run_id: String, status: String, reason: Option<String> }
//! ```
//!
//! - The run is created by `started_by` with the plan's hash pinned; every
//!   later transition is `System(engine)`'s.
//! - Per step, in order: the step's start and the call's reservation (zero)
//!   are committed, each with its audit entry, before the provider is
//!   called; the call's settlement and the step's finish commit together;
//!   then the run's finish.
//! - One turn per step: the request is the plan's system instructions, the
//!   step's instruction as the one user part, the model and
//!   `max_output_tokens`, and no tools. `EndTurn` stores the text as the
//!   step's output. A provider error, a tool request, `MaxTokens` or
//!   `Cancelled` fails the step and the run (`status` is `failed`, `reason`
//!   says why, later steps stay queued); that is an `Ok` outcome carrying
//!   the run id, not an error.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use am_domain::{
    Actor, BoxFuture, CancelSignal, ChatOutcome, ChatRequest, EventSink, Instruction, LlmProvider,
    Message, ModelCapabilities, Part, ProviderError, Role, StopReason, Taint, TrustedInput, Usage,
};
use am_engine::run::{HandOff, Limits, RunError, RunPlan, StepPlan, run_plan};
use am_store::{CallRow, LedgerRow, Reader, RunRow, StepRow, Store, WriteCommand, Writer};
use am_testkit::{FakeLlm, ScriptedTurn};
use tokio_util::sync::CancellationToken;

/// The pinned snapshot's hash: 64 lowercase hex characters.
const HASH: &str = "0a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9";

fn owner() -> Actor {
    Actor::Human {
        person: "owner".into(),
        hat: "Owner".into(),
    }
}

/// A store on a bare path in a temporary folder.
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

fn step(
    key: &str,
    model: &str,
    max_output_tokens: u32,
    provider: Arc<dyn LlmProvider>,
) -> StepPlan {
    StepPlan {
        key: key.into(),
        // Story 3.3: the step's seat (a tool call's `Actor::Seat`), its
        // plan inputs, its hand-off and its loop caps; here the defaults.
        seat: format!("seat-{key}"),
        system: vec![Instruction::authored(format!(
            "You are the seat of step {key}."
        ))],
        instruction: Instruction::authored(format!("Do the work of step {key}.")),
        inputs: vec![],
        hand_off: HandOff::Next,
        limits: Limits::default(),
        model: model.into(),
        max_output_tokens,
        provider,
        prompt: am_engine::prompt::StepPrompt::default(),
        output_schema: None,
        // Story 3.6: no grant, no table, no document, no row read.
        grants: Default::default(),
        seat_team: "core".into(),
        table: None,
        document: None,
    }
}

fn plan(steps: Vec<StepPlan>) -> RunPlan {
    RunPlan {
        workflow: "wf".into(),
        config_hash: HASH.into(),
        steps,
        clock: am_testkit::FakeClock::at("2026-11-24T10:00:00Z"),
        row_tool: None,
    }
}

/// The one request a step makes: its system instructions, its instruction
/// as the only user part, its model and output limit, and no tools.
fn request_of(step: &StepPlan) -> ChatRequest {
    ChatRequest {
        model: step.model.clone(),
        system: step.system.clone(),
        messages: vec![Message {
            role: Role::User,
            parts: vec![Part::Instruction(step.instruction.clone())],
        }],
        tools: vec![],
        max_output_tokens: step.max_output_tokens,
    }
}

/// The audit log's lines (JSON Lines), without the migrations' entries.
fn audit_lines(reader: &Reader) -> Vec<String> {
    let mut out = Vec::new();
    reader.export_audit_jsonl(&mut out).unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .filter(|l| field(l, "action").as_deref() != Some("store.migrated"))
        .map(str::to_string)
        .collect()
}

/// The text value of `key` on an export line.
fn field(line: &str, key: &str) -> Option<String> {
    let start = line.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let end = line[start..].find('"')?;
    Some(line[start..start + end].to_string())
}

fn actions(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|l| field(l, "action").expect("an action"))
        .collect()
}

fn by_owner(line: &str) -> bool {
    line.contains("\"kind\":\"human\"") && line.contains("\"hat\":\"Owner\"")
}

fn by_engine(line: &str) -> bool {
    line.contains("\"kind\":\"system\"") && line.contains("\"component\":\"engine\"")
}

/// What the journal held at one moment.
#[derive(Debug, Clone)]
struct Seen {
    runs: Vec<RunRow>,
    steps: Vec<StepRow>,
    calls: Vec<CallRow>,
    ledger: Vec<LedgerRow>,
    audit: Vec<String>,
}

fn journal(db: &Path) -> Seen {
    let reader = Reader::open(db).unwrap();
    let runs = reader.runs().unwrap();
    let (mut steps, mut calls, mut ledger) = (Vec::new(), Vec::new(), Vec::new());
    for run in &runs {
        steps.extend(reader.steps(&run.id).unwrap());
        calls.extend(reader.calls_for_run(&run.id).unwrap());
        ledger.extend(reader.ledger_for_run(&run.id).unwrap());
    }
    Seen {
        runs,
        steps,
        calls,
        ledger,
        audit: audit_lines(&reader),
    }
}

fn text_reply(text: &str, usage: Option<Usage>) -> ChatOutcome {
    ChatOutcome {
        text: text.into(),
        tool_calls: vec![],
        usage,
        stop: StopReason::EndTurn,
    }
}

/// A provider that reads the journal, through its own read-only
/// connection, at the moment it is called, then gives its fixed outcome.
struct Spy {
    db: PathBuf,
    reply: ChatOutcome,
    seen: Mutex<Vec<Seen>>,
    requests: Mutex<Vec<ChatRequest>>,
}

impl Spy {
    fn new(db: &Path, reply: ChatOutcome) -> Arc<Spy> {
        Arc::new(Spy {
            db: db.to_path_buf(),
            reply,
            seen: Mutex::new(Vec::new()),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    fn requests(&self) -> Vec<ChatRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl LlmProvider for Spy {
    fn account_id(&self) -> String {
        "spy".into()
    }
    fn capabilities(&self, _model: &str) -> ModelCapabilities {
        ModelCapabilities::default()
    }
    fn check_file_parts(&self, _request: &ChatRequest) -> Result<(), ProviderError> {
        Ok(())
    }
    fn stream(
        &self,
        request: ChatRequest,
        _cancel: Arc<dyn CancelSignal>,
        _sink: Arc<dyn EventSink>,
    ) -> BoxFuture<Result<ChatOutcome, ProviderError>> {
        self.seen.lock().unwrap().push(journal(&self.db));
        self.requests.lock().unwrap().push(request);
        let reply = self.reply.clone();
        Box::pin(async move { Ok(reply) })
    }
}

// ------------------------------------------------------------------ run

/// Run and After: one step on a scripted "done" with usage. The run
/// succeeds; the step holds the text as its output; the call is settled
/// with the usage against a zero reservation; the run row holds the plan's
/// hash; both have their times; the audit log ends with the settlement and
/// the step's finish in one entry, then the run's finish, and verifies.
#[tokio::test]
async fn one_step_succeeds_and_the_journal_holds_it() {
    let p = project();
    let llm = Arc::new(FakeLlm::new(vec![ScriptedTurn::text("done").usage(12, 3)]));
    let plan = plan(vec![step("only", "example-local", 512, llm.clone())]);

    let outcome = run_plan(&p.store, owner(), "manual", &plan, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(outcome.status, "succeeded");
    assert_eq!(outcome.reason, None);
    assert_eq!(llm.requests(), vec![request_of(&plan.steps[0])]);

    let now = journal(&p.db);
    assert_eq!(now.runs.len(), 1);
    let run = &now.runs[0];
    assert_eq!(run.id, outcome.run_id);
    assert_eq!(
        (run.workflow.as_str(), run.status.as_str()),
        ("wf", "succeeded")
    );
    assert_eq!(run.config_hash.as_deref(), Some(HASH));
    assert!(run.finished_at.is_some(), "{run:?}");

    assert_eq!(now.steps.len(), 1);
    let s = &now.steps[0];
    assert_eq!(
        (s.step_key.as_str(), s.status.as_str()),
        ("only", "succeeded")
    );
    assert_eq!(s.output.as_deref(), Some("done"));
    assert!(s.started_at.is_some() && s.finished_at.is_some(), "{s:?}");

    assert_eq!(now.calls.len(), 1);
    let c = &now.calls[0];
    assert!(c.settled, "{c:?}");
    assert_eq!((c.step_run_id.as_str(), c.call_seq), (s.id.as_str(), 1));
    assert_eq!((c.input_tokens, c.output_tokens), (Some(12), Some(3)));
    // A zero reservation, settled by the store's own rules (story 7 brings
    // amounts).
    assert_eq!(now.ledger.len(), 1);
    assert_eq!(now.ledger[0].reserved_micro_usd, 0);
    assert_eq!(now.ledger[0].state, "settled");
    assert_eq!(now.ledger[0].settled_micro_usd, Some(0));

    assert_eq!(
        actions(&now.audit),
        vec![
            "run.created",
            "step.started",
            "model_call.reserved",
            "model_call.settled+step.finished",
            "run.finished",
        ]
    );
    // The owner started it; the engine did the rest.
    assert!(by_owner(&now.audit[0]), "{}", now.audit[0]);
    assert!(
        now.audit[0].contains(&format!("\"config_hash\":\"{HASH}\"")),
        "the hash is audited as an argument: {}",
        now.audit[0]
    );
    assert!(now.audit[0].contains("\"origin\":\"manual\""));
    for line in &now.audit[1..] {
        assert!(by_engine(line), "{line}");
    }
    assert_eq!(
        field(&now.audit[0], "target").as_deref(),
        Some(run.id.as_str())
    );
    assert_eq!(
        p.store.reader().unwrap().verify_audit_chain().unwrap(),
        None
    );
}

/// Journal order (AD-8): when the provider is called, the run row
/// (`running`, with the hash), the step's `running` state with its start
/// time and the call's reservation (not settled) are already committed and
/// readable from another connection, each with its audit entry, in order;
/// nothing of the call's settlement, the step's finish or the run's finish
/// is.
#[tokio::test]
async fn every_transition_is_committed_before_the_provider_is_called() {
    let p = project();
    let spy = Spy::new(
        &p.db,
        text_reply(
            "done",
            Some(Usage {
                input_tokens: 7,
                output_tokens: 2,
            }),
        ),
    );
    let plan = plan(vec![step("only", "example-local", 4096, spy.clone())]);

    let outcome = run_plan(&p.store, owner(), "manual", &plan, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(outcome.status, "succeeded");

    let seen = spy.seen();
    assert_eq!(seen.len(), 1, "one turn per step");
    let at_call = &seen[0];
    assert_eq!(at_call.runs.len(), 1);
    assert_eq!(at_call.runs[0].id, outcome.run_id);
    assert_eq!(at_call.runs[0].status, "running");
    assert_eq!(at_call.runs[0].config_hash.as_deref(), Some(HASH));
    assert_eq!(at_call.runs[0].finished_at, None);
    assert_eq!(at_call.steps.len(), 1);
    assert_eq!(at_call.steps[0].status, "running");
    assert!(at_call.steps[0].started_at.is_some(), "{:?}", at_call.steps);
    assert_eq!(at_call.steps[0].finished_at, None);
    assert_eq!(at_call.steps[0].output, None);
    assert_eq!(at_call.calls.len(), 1);
    assert!(!at_call.calls[0].settled, "{:?}", at_call.calls);
    assert_eq!(
        (
            at_call.calls[0].input_tokens,
            at_call.calls[0].output_tokens
        ),
        (None, None)
    );
    assert_eq!(at_call.ledger.len(), 1);
    assert_eq!(at_call.ledger[0].state, "reserved");
    assert_eq!(at_call.ledger[0].reserved_micro_usd, 0);
    assert_eq!(at_call.ledger[0].settled_micro_usd, None);
    assert_eq!(
        actions(&at_call.audit),
        vec!["run.created", "step.started", "model_call.reserved"]
    );
    assert!(by_owner(&at_call.audit[0]), "{}", at_call.audit[0]);
    assert!(by_engine(&at_call.audit[1]), "{}", at_call.audit[1]);
    assert!(by_engine(&at_call.audit[2]), "{}", at_call.audit[2]);

    // After: the rest of the journal, appended to what was there.
    let after = journal(&p.db);
    assert_eq!(after.audit[..3], at_call.audit[..]);
    assert_eq!(
        actions(&after.audit)[3..],
        ["model_call.settled+step.finished", "run.finished"]
    );
    assert!(after.calls[0].settled);
    assert_eq!(
        (after.calls[0].input_tokens, after.calls[0].output_tokens),
        (Some(7), Some(2))
    );
    assert_eq!(after.steps[0].started_at, at_call.steps[0].started_at);
    assert!(after.steps[0].finished_at.is_some());
    assert!(after.runs[0].finished_at.is_some());
    assert_eq!(
        p.store.reader().unwrap().verify_audit_chain().unwrap(),
        None
    );
}

/// Two steps: each runs in order with its own provider, system
/// instructions, instruction, model and output limit; when the second
/// provider is called the first step has finished and its call is settled.
/// Story 3.3 (matrix row 2, Question 4 (a)) supersedes 3.1's "holds nothing
/// of the first step's output": by `next`, the second request holds the
/// first step's output as one trusted input labelled `step:gather@1`,
/// after its instruction (the first step is untainted).
#[tokio::test]
async fn two_steps_run_in_order_each_with_its_own_request() {
    let p = project();
    let first = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("first out").usage(5, 2),
    ]));
    let second = Spy::new(&p.db, text_reply("second out", None));
    let plan = plan(vec![
        step("gather", "model-one", 4096, first.clone()),
        step("summarise", "model-two", 256, second.clone()),
    ]);

    let outcome = run_plan(&p.store, owner(), "manual", &plan, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(outcome.status, "succeeded");
    assert_eq!(first.requests(), vec![request_of(&plan.steps[0])]);
    let mut after_first = request_of(&plan.steps[1]);
    after_first.messages[0]
        .parts
        .push(TrustedInput::from_step_output(
            "step:gather@1",
            "first out",
            Taint::Clean,
        ));
    assert_eq!(second.requests(), vec![after_first]);

    let seen = second.seen();
    assert_eq!(seen.len(), 1);
    let at_second = &seen[0];
    assert_eq!(
        at_second
            .steps
            .iter()
            .map(|s| (s.step_key.as_str(), s.status.as_str()))
            .collect::<Vec<_>>(),
        vec![("gather", "succeeded"), ("summarise", "running")]
    );
    assert_eq!(at_second.steps[0].output.as_deref(), Some("first out"));
    assert_eq!(
        at_second
            .calls
            .iter()
            .map(|c| c.settled)
            .collect::<Vec<_>>(),
        vec![true, false]
    );
    assert_eq!(at_second.runs[0].status, "running");

    let after = journal(&p.db);
    assert_eq!(
        after
            .steps
            .iter()
            .map(|s| (
                s.seq,
                s.step_key.as_str(),
                s.status.as_str(),
                s.output.as_deref()
            ))
            .collect::<Vec<_>>(),
        vec![
            (0, "gather", "succeeded", Some("first out")),
            (1, "summarise", "succeeded", Some("second out")),
        ]
    );
    assert_eq!(after.calls.len(), 2);
    assert!(after.calls.iter().all(|c| c.settled && c.call_seq == 1));
    assert_eq!(
        (
            after.calls[0].step_run_id.as_str(),
            after.calls[1].step_run_id.as_str()
        ),
        (after.steps[0].id.as_str(), after.steps[1].id.as_str())
    );
    // No usage reported: the call settles at its (zero) reservation.
    assert_eq!(
        (after.calls[1].input_tokens, after.calls[1].output_tokens),
        (None, None)
    );
    assert_eq!(after.runs[0].status, "succeeded");
    assert_eq!(
        actions(&after.audit),
        vec![
            "run.created",
            "step.started",
            "model_call.reserved",
            "model_call.settled+step.finished",
            "step.started",
            "model_call.reserved",
            "model_call.settled+step.finished",
            "run.finished",
        ]
    );
    assert!(after.audit[0].contains("\"steps\":2"), "{}", after.audit[0]);
    assert_eq!(
        p.store.reader().unwrap().verify_audit_chain().unwrap(),
        None
    );
}

// ------------------------------------------------------------------ failures

/// Provider fails: the step and the run are `failed`, the outcome carries
/// the run id and the provider's words, the call is settled by the store's
/// rule for a call with no usage (at its zero reservation), the next step
/// stays queued and its provider is never called.
#[tokio::test]
async fn a_provider_error_fails_the_step_and_the_run() {
    let p = project();
    let failing = Arc::new(FakeLlm::new(vec![ScriptedTurn::fail(
        ProviderError::Http {
            status: 500,
            body: "upstream is down".into(),
        },
    )]));
    let never = Arc::new(FakeLlm::new(vec![ScriptedTurn::text("never")]));
    let plan = plan(vec![
        step("gather", "model-one", 4096, failing.clone()),
        step("summarise", "model-two", 4096, never.clone()),
    ]);

    let outcome = run_plan(&p.store, owner(), "manual", &plan, CancellationToken::new())
        .await
        .expect("a failed run is an outcome, not an error");

    assert_eq!(outcome.status, "failed");
    let reason = outcome.reason.clone().expect("the reason, in words");
    assert!(reason.contains("500"), "{reason}");
    assert_eq!(failing.calls(), 1, "no retry in this story");
    assert_eq!(never.calls(), 0, "the next step never runs");

    let after = journal(&p.db);
    assert_eq!(after.runs.len(), 1);
    assert_eq!(after.runs[0].id, outcome.run_id);
    assert_eq!(after.runs[0].status, "failed");
    assert!(after.runs[0].finished_at.is_some());
    assert_eq!(
        after
            .steps
            .iter()
            .map(|s| (s.step_key.as_str(), s.status.as_str()))
            .collect::<Vec<_>>(),
        vec![("gather", "failed"), ("summarise", "queued")]
    );
    assert!(after.steps[0].started_at.is_some() && after.steps[0].finished_at.is_some());
    assert_eq!(
        (
            after.steps[1].started_at.clone(),
            after.steps[1].finished_at.clone()
        ),
        (None, None)
    );
    assert_eq!(after.calls.len(), 1);
    assert!(
        after.calls[0].settled,
        "no call is left open: {:?}",
        after.calls
    );
    assert_eq!(
        (after.calls[0].input_tokens, after.calls[0].output_tokens),
        (None, None)
    );
    assert_eq!(after.ledger[0].state, "settled");
    assert_eq!(after.ledger[0].settled_micro_usd, Some(0));
    assert_eq!(
        actions(&after.audit),
        vec![
            "run.created",
            "step.started",
            "model_call.reserved",
            "model_call.settled+step.finished",
            "run.finished",
        ]
    );
    assert!(
        after.audit[3].contains("\"status\":\"failed\"")
            && after.audit[4].contains("\"status\":\"failed\""),
        "{:?}",
        &after.audit[3..]
    );
    assert_eq!(
        p.store.reader().unwrap().verify_audit_chain().unwrap(),
        None
    );
}

/// Tool request, superseded by story 3.6 (AD-11): every tool request goes
/// through ToolGate. The seat is granted nothing, so a turn that asks for
/// `web_search` is refused in ToolGate's words (not granted); the call is
/// journaled `refused` and audited `toolgate.refused`; the step and the run
/// fail and say so; the request offered no tool.
#[tokio::test]
async fn a_tool_request_is_refused_by_toolgate_when_the_seat_is_not_granted_it() {
    let p = project();
    let llm = Arc::new(FakeLlm::new(vec![
        ScriptedTurn::text("let me look that up")
            .tool_call("web_search", "{\"query\":\"x\"}")
            .usage(9, 4),
    ]));
    let plan = plan(vec![step("only", "example-local", 4096, llm.clone())]);

    let outcome = run_plan(&p.store, owner(), "manual", &plan, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(outcome.status, "failed");
    let reason = outcome.reason.clone().expect("the reason, in words");
    assert!(
        reason.contains("web_search")
            && reason.contains("not granted")
            && reason.contains("seat-only"),
        "{reason}"
    );
    assert_eq!(llm.calls(), 1, "one turn, no tool loop");
    assert!(llm.requests()[0].tools.is_empty());

    let after = journal(&p.db);
    assert_eq!(after.steps[0].status, "failed");
    assert_ne!(
        after.steps[0].output.as_deref(),
        Some("let me look that up"),
        "a failed turn's text is not the step's output"
    );
    assert_eq!(after.runs[0].status, "failed");
    // The usage the provider reported is still settled.
    assert!(after.calls[0].settled);
    assert_eq!(
        (after.calls[0].input_tokens, after.calls[0].output_tokens),
        (Some(9), Some(4))
    );
    let reader = Reader::open(&p.db).unwrap();
    let tools = reader.tool_calls(&after.steps[0].id).unwrap();
    assert_eq!(tools.len(), 1, "{tools:#?}");
    assert_eq!(
        (tools[0].tool.as_str(), tools[0].status.as_str()),
        ("web_search", "refused")
    );
    let actions = actions(&after.audit);
    assert!(
        actions.iter().any(|a| a == "toolgate.refused"),
        "{actions:?}"
    );
    assert_eq!(actions.last().map(String::as_str), Some("run.finished"));
}

/// `MaxTokens` and `Cancelled`: a turn cut off at its output limit, or
/// cancelled, fails the step and the run with the reason in words; its
/// partial text is not stored as the step's output.
#[tokio::test]
async fn a_cut_off_or_cancelled_turn_fails_the_step() {
    for (stop, word) in [
        (StopReason::MaxTokens, "token"),
        (StopReason::Cancelled, "cancel"),
    ] {
        let p = project();
        let spy = Spy::new(
            &p.db,
            ChatOutcome {
                text: "a partial ans".into(),
                tool_calls: vec![],
                usage: None,
                stop,
            },
        );
        let plan = plan(vec![step("only", "example-local", 16, spy.clone())]);

        let outcome = run_plan(&p.store, owner(), "manual", &plan, CancellationToken::new())
            .await
            .unwrap();

        assert_eq!(outcome.status, "failed", "{stop:?}");
        let reason = outcome
            .reason
            .clone()
            .expect("the reason, in words")
            .to_lowercase();
        assert!(reason.contains(word), "{stop:?}: {reason}");
        assert_eq!(spy.requests().len(), 1, "{stop:?}: no retry");
        let after = journal(&p.db);
        assert_eq!(after.steps[0].status, "failed", "{stop:?}");
        assert_ne!(
            after.steps[0].output.as_deref(),
            Some("a partial ans"),
            "{stop:?}"
        );
        assert_eq!(after.runs[0].status, "failed", "{stop:?}");
        assert!(after.runs[0].finished_at.is_some(), "{stop:?}");
        assert!(after.calls[0].settled, "{stop:?}");
        assert_eq!(
            actions(&after.audit),
            vec![
                "run.created",
                "step.started",
                "model_call.reserved",
                "model_call.settled+step.finished",
                "run.finished",
            ],
            "{stop:?}"
        );
    }
}

// ------------------------------------------------------------------ store errors

/// A provider that, when it is called, settles its own call through the
/// store's writer (as another writer of the journal would), then answers.
/// The engine's settlement of that call is then refused by the store: the
/// seam for a store error after the run was created.
struct SettlesItsOwnCall {
    db: PathBuf,
    writer: Writer,
}

impl LlmProvider for SettlesItsOwnCall {
    fn account_id(&self) -> String {
        "saboteur".into()
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
        let open = journal(&self.db)
            .calls
            .into_iter()
            .find(|c| !c.settled)
            .expect("the call is reserved before the provider is called");
        self.writer
            .exec(
                Actor::System {
                    component: "test".into(),
                },
                WriteCommand::SettleCall {
                    call_id: open.id,
                    usage: None,
                    cost_micro_usd: None,
                },
            )
            .unwrap();
        Box::pin(async { Ok(text_reply("done", None)) })
    }
}

/// A store error after the run was created (review of story 3.1): the
/// engine tries once to finish the open step and the run as `failed`, and
/// the error it returns carries the run's id, the status the journal now
/// holds and the store's own error, so the caller can name the run. No run
/// is left `running`; the next step stays queued and is never called.
#[tokio::test]
async fn a_store_error_after_the_run_was_created_fails_the_run_and_names_it() {
    let p = project();
    let saboteur = Arc::new(SettlesItsOwnCall {
        db: p.db.clone(),
        writer: p.store.writer(),
    });
    let never = Arc::new(FakeLlm::new(vec![ScriptedTurn::text("never")]));
    let plan = plan(vec![
        step("gather", "model-one", 4096, saboteur),
        step("summarise", "model-two", 4096, never.clone()),
    ]);

    let stopped = run_plan(&p.store, owner(), "manual", &plan, CancellationToken::new())
        .await
        .expect_err("the store refused the settlement");

    let after = journal(&p.db);
    assert_eq!(after.runs.len(), 1);
    let RunError::Stopped {
        run_id,
        status,
        reason,
        source,
    } = &stopped
    else {
        panic!("a store error after the run was created names the run: {stopped:?}");
    };
    assert_eq!(run_id, &after.runs[0].id);
    assert_eq!(status, "failed", "the engine could still finish the run");
    assert_eq!(reason, &None, "no step failed for a reason of its own");
    assert!(
        source.to_string().contains("is not open"),
        "the store's own error: {source}"
    );
    assert!(stopped.to_string().contains(run_id.as_str()), "{stopped}");
    assert_eq!(never.calls(), 0, "the next step never runs");

    assert_eq!(after.runs[0].status, "failed");
    assert!(after.runs[0].finished_at.is_some());
    assert_eq!(
        after
            .steps
            .iter()
            .map(|s| (s.step_key.as_str(), s.status.as_str()))
            .collect::<Vec<_>>(),
        vec![("gather", "failed"), ("summarise", "queued")]
    );
    assert!(after.steps[0].finished_at.is_some());
    assert_eq!(after.steps[0].output, None, "the reply was not journaled");
    assert_eq!(
        actions(&after.audit),
        vec![
            "run.created",
            "step.started",
            "model_call.reserved",
            "model_call.settled",
            "step.finished",
            "run.finished",
        ]
    );
    assert!(by_engine(&after.audit[4]) && by_engine(&after.audit[5]));
    assert!(
        after.audit[4].contains("\"status\":\"failed\"")
            && after.audit[5].contains("\"status\":\"failed\""),
        "{:?}",
        &after.audit[4..]
    );
    assert_eq!(
        p.store.reader().unwrap().verify_audit_chain().unwrap(),
        None
    );
}
