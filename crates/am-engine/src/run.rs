//! The run driver (epic 3 stories 1 and 3; AD-8, AD-6, AD-10, AD-11,
//! AD-16, AD-27, AD-28, AD-36): a plan of steps in order, every transition
//! committed before the effect it authorises.
//!
//! The plan is plain types: am-app maps a config snapshot to it and
//! resolves each step's provider (AD-4; this crate reads no config). The
//! driver writes each command with its audit entry in one transaction: the
//! run (`running`, with the plan's snapshot hash pinned) as the Actor who
//! started it, and every later transition as `System(engine)`.
//!
//! **The loop decides from the journal.** After the run is created, the
//! driver reads the run's step attempts back each time it moves on and runs
//! the first step (in plan order) whose latest attempt is `queued`; never
//! an in-memory index, so stories 3.4 and 3.9 can enter it later.
//!
//! **A step attempt (story 3.3)** starts with its inputs journaled
//! (`step_inputs`) and its taint committed, then is a bounded loop of model
//! turns (AD-11, RULE-11): each turn's call is reserved before it is made;
//! a turn that asks for tools goes on once ToolGate has decided each call
//! and each allowed call's result is appended (as `UntrustedData`, which
//! taints the attempt with the next reservation). Three caps end the
//! attempt `failed` with one Failure Inbox item: `iterations` (model turns,
//! counted from `model_calls` rows), `tool_calls` (counted from `tool_calls`
//! rows) and `wall_clock` (a fresh attempt's deadline is tokio's clock
//! right after its `StartAttempt` plus the cap; story 3.9's resume will
//! count a resumed attempt's persisted `started_at`). Nothing starts once
//! the deadline has passed, and a reply or tool ready at the deadline loses
//! to it; the call in flight is cancelled through its own child token and
//! settles by the ledger rule.
//!
//! **Inputs and taint (AD-10, AD-27, AD-36).** By `next` a step receives
//! the previous step's committed output as one input labelled
//! `step:<key>@<attempt>`; a loop-back target receives the reasons labelled
//! `loop-back:<key>@<attempt>` (its `loop_back` input, journaled by the
//! loop-back itself); a plan input is untrusted. Each goes through
//! `TrustedInput::from_step_output` with the producer's taint as committed
//! in the journal: a clean producer's text is a `TrustedInput`, a tainted
//! one's is fenced `UntrustedData` that taints the receiver. The request's
//! one user message holds the instruction, then the trusted inputs, then
//! the fenced ones.
//!
//! **Hand-offs (AD-36).** `next` is the default. A `loop_back` step's final
//! message must be a JSON object whose named field is a list of strings
//! (fail closed until story 3.5's schemas): an empty list is `next`; a
//! non-empty one ends the attempt `looped_back` and queues new attempts
//! from the target through the producer, until the producer's `looped_back`
//! attempts in the journal reach `max`, when the step fails with a Failure
//! item. A `gate` step makes no model call: it waits (`waiting_human`) with
//! one Review item whose payload is the previous step's attempt (and its
//! `max_returns`), and the run stays `running` (the outcome says
//! `waiting_human`). Only a Human resolve (story 3.4) moves it on.
//!
//! **After a person decides (story 3.4; AD-8, AD-10, AD-15, AD-16, AD-36).**
//! [`continue_run`] takes a run that stopped cleanly: still `running`, its
//! plan built from the snapshot the run pinned, no call unsettled and no
//! step `running` but a gate whose Review a person resolved. It applies that
//! decision first ([`crate::gate::apply_resolution`], as `System(engine)`),
//! then enters the same loop. The step after an approved gate receives the
//! Review's payload (the producer's output, with its committed taint) as
//! its `next` input; a returned producer's new attempt receives the
//! person's note as a trusted part, built with
//! `TrustedInput::from_owner_note` from the resolver the journal holds (no
//! person, no note, and the attempt does not start).
//!
//! **Tools.** [`run_plan`] takes none: a tool request fails the step in
//! story 3.1's words. Tools run only through [`run_plan_with_tools`]'s
//! runner, a constructor parameter shipped code never passes (owner
//! condition 2, `cargo xtask seam-guard`), and only ToolGate calls it, for
//! a call it admitted (read tools only in this story); the driver holds the
//! gate, never the runner (review R7). A tool name the model chose and no
//! manifest holds is labelled by ToolGate and journaled, audited and
//! reported only as `unknown:<sha256 of the name>` (review P15, R9).
//!
//! A provider error, a reply cut off at its output limit or a cancelled
//! call fails the step and the run with the reason in words and no item
//! (story 3.8 adds Failure items for those); the failed turn's text is not
//! stored, and later steps stay queued.
//!
//! Not here: full labelled prompt assembly (story 5), amounts (story 7: the
//! reservation is zero), retries and fallback (story 8), resume (story 9).
//! The spike's resumable driver stays in [`crate::journal`].

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use am_domain::{
    Actor, AuditEvent, AuditSink, ChatOutcome, ChatRequest, EventSink, InboxKind, Instruction,
    LlmProvider, Message, Part, ProviderError, Role, StepStatus, StopReason, StreamEvent, Taint,
    ToolCall, ToolRunner, ToolSpec, TrustedInput, UntrustedData,
};
use am_store::{
    CallSettlement, NewInboxItem, Reader, StepInput, StepRow, Store, StoreError, WriteCommand,
    new_id,
};
use tokio_util::sync::CancellationToken;

use crate::TokenSignal;
use crate::toolgate::{ToolGate, builtin_names, label_of};

/// The rate-snapshot id of a zero reservation: no price was read (amounts
/// arrive with the cost ledger, story 7).
const NO_RATE_SNAPSHOT: &str = "none";

/// How long a call cancelled at its wall-clock cap may take to stop before
/// it is settled at its reservation anyway.
const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// How a step hands its work on (AD-36).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandOff {
    /// The following step runs and receives this step's output.
    Next,
    /// When this step's final message lists reasons in `field`, the run goes
    /// back to the earlier step `to`, at most `max` times.
    LoopBack { to: String, field: String, max: u32 },
    /// A human gate: no model call; the step waits with a Review item. The
    /// owner may return its work `max_returns` times (story 3.4, AD-36);
    /// past that, the gate fails with a Failure item.
    Gate { max_returns: u32 },
}

/// A step attempt's caps (AD-11, RULE-11). The defaults are the owner's
/// placeholders (story 3.3, Question 7); am-app sets each step's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Model turns per attempt.
    pub iterations: u32,
    /// Tool calls per attempt.
    pub tool_calls: u32,
    /// Time per attempt, from its persisted start.
    pub wall_clock: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            iterations: 8,
            tool_calls: 20,
            wall_clock: Duration::from_secs(600),
        }
    }
}

/// One step: a bounded loop of model turns on `provider`, or, for a gate,
/// a wait for a person (its provider is never called).
#[derive(Clone)]
pub struct StepPlan {
    /// The step's key in the run (its id in the workflow).
    pub key: String,
    /// The seat that runs it: a tool call's `Actor::Seat`.
    pub seat: String,
    /// The seat's instructions, as the request's system part.
    pub system: Vec<Instruction>,
    /// The step's instruction, as the first user part.
    pub instruction: Instruction,
    /// Inputs the plan hands the step (untrusted; journaled `untrusted`,
    /// and they taint every attempt of the step).
    pub inputs: Vec<UntrustedData>,
    pub hand_off: HandOff,
    pub limits: Limits,
    /// The model id on `provider`'s Account.
    pub model: String,
    pub max_output_tokens: u32,
    /// A gate step's provider is never called.
    pub provider: Arc<dyn LlmProvider>,
}

/// A run to make: the workflow's steps in order, built from one config
/// snapshot.
#[derive(Clone)]
pub struct RunPlan {
    pub workflow: String,
    /// The hash of the config snapshot the plan was built from (AD-16),
    /// pinned on the run row.
    pub config_hash: String,
    pub steps: Vec<StepPlan>,
}

/// How a run ended, or where it waits. A failed run is an outcome, not an
/// error: the caller still has its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub run_id: String,
    /// `succeeded`, `failed` or `waiting_human` (the run row stays
    /// `running` while a step waits).
    pub status: String,
    /// Why the run failed, in words; `None` when it succeeded or waits.
    pub reason: Option<String>,
    /// The Inbox item the run stopped at: the Review item a waiting run
    /// waits on, or the Failure item a failed run raised.
    pub item: Option<String>,
}

/// Why a run could not be made or journaled.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("the workflow {0} has no steps, so there is nothing to run")]
    NoSteps(String),
    /// The tools handed to the run cannot be used (story 3.3, review R7).
    #[error("the run's tools were refused: {0}")]
    Tools(String),
    /// The run could not be created: the journal holds nothing of it.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The store failed a command after the run was created. The engine
    /// tried once to record how the run ended: the status it was about to
    /// record when every started step had finished, `failed` (with the
    /// open step) when one had not. `status` is what the journal holds
    /// now: that status, or `running` when the store refused that too.
    /// `reason` is why a step failed, when one did before the store erred.
    #[error("run {run_id} stopped ({status}): {source}")]
    Stopped {
        run_id: String,
        status: String,
        reason: Option<String>,
        source: Box<StoreError>,
    },
    /// Story 3.4 (AD-16, Question 2): a run goes on only on the snapshot it
    /// pinned; nothing was written.
    #[error(
        "the project's config is not the snapshot the run pinned: the run pinned {pinned}, and the project's files now give {now}; a run goes on only on the config it started with (AD-16), so nothing was written. Put the files back as they were when the run started, or reject the item and start a new run"
    )]
    ConfigChanged { pinned: String, now: String },
    /// Story 3.4: the run did not stop cleanly (a step left running, a call
    /// unsettled) or has finished; nothing was written.
    #[error("{0}")]
    NotContinuable(String),
}

fn engine() -> Actor {
    Actor::System {
        component: "engine".into(),
    }
}

struct NullSink;

impl EventSink for NullSink {
    fn on_event(&self, _event: StreamEvent) {}
}

/// Why a turn did not finish its step, in words; `None` for a turn that
/// ended its turn or asked for tools.
fn failure_of(step: &StepPlan, turn: &Result<ChatOutcome, ProviderError>) -> Option<String> {
    let out = match turn {
        Ok(out) => out,
        Err(e) => return Some(format!("the model call failed: {e}")),
    };
    match out.stop {
        StopReason::Cancelled => Some("the model call was cancelled before it finished".into()),
        StopReason::MaxTokens => Some(format!(
            "the reply was cut off at the step's output limit of {} tokens (max_tokens)",
            step.max_output_tokens
        )),
        StopReason::ToolUse if out.tool_calls.is_empty() => {
            Some("the model stopped to use a tool but named none".into())
        }
        StopReason::ToolUse => None,
        _ if !out.tool_calls.is_empty() => None,
        StopReason::EndTurn => None,
        StopReason::Other => Some("the model stopped before it finished its turn".into()),
    }
}

/// Story 3.1's words for a tool request with no tools: each name as
/// ToolGate's labelling gives it (a built-in id as itself, any other as
/// `unknown:<sha256>`; review P15, R9).
fn tool_words(out: &ChatOutcome) -> String {
    let known = builtin_names();
    let names: Vec<String> = out
        .tool_calls
        .iter()
        .map(|c| format!("`{}`", label_of(&c.name, &known)))
        .collect();
    let asked = if names.is_empty() {
        "a tool".to_string()
    } else {
        names.join(", ")
    };
    format!("the model asked for {asked}, and tools are not available in this version")
}

/// A duration in its shortest whole unit: `2h`, `10m`, `30s`.
fn duration_words(d: Duration) -> String {
    let s = d.as_secs();
    if s > 0 && s.is_multiple_of(3600) {
        format!("{}h", s / 3600)
    } else if s > 0 && s.is_multiple_of(60) {
        format!("{}m", s / 60)
    } else {
        format!("{s}s")
    }
}

/// The function that executes one write command (the store's writer).
type Exec<'a> = &'a (dyn Fn(Actor, WriteCommand) -> Result<(), StoreError> + Send + Sync);

/// How far a run got, for the cleanup after a store error.
#[derive(Default)]
struct Progress {
    /// The step run that is started and not finished.
    open_step: Option<String>,
    /// Why a step failed, from the moment its turn's failure is known.
    reason: Option<String>,
    /// The status the engine is about to record for the run, once every
    /// started step has finished: `succeeded` after the last step, `failed`
    /// after a failed one.
    finishing: Option<&'static str>,
    /// The Inbox item the run stopped at.
    item: Option<String>,
}

impl Progress {
    /// What the cleanup records for the run: what the engine was about to
    /// record, or `failed` when a step did not finish.
    fn to_record(&self) -> &'static str {
        match (&self.open_step, self.finishing) {
            (None, Some(status)) => status,
            _ => "failed",
        }
    }
}

/// Creates the run for `plan` as `started_by` and drives it to its end, or
/// to a gate where it waits. `origin` says what started it (`manual`). No
/// tool runs: a tool request fails its step in words. See the module docs
/// for the order of the commits.
///
/// A store error once the run exists never leaves it `running` unsaid: the
/// engine tries once to record how the run ended (see
/// [`RunError::Stopped`]) and returns that error with the run's id, the
/// status the journal now holds and the failed step's reason, if any. (A
/// call the store could not settle stays as the journal has it.)
pub async fn run_plan(
    store: &Store,
    started_by: Actor,
    origin: &str,
    plan: &RunPlan,
    cancel: CancellationToken,
) -> Result<RunOutcome, RunError> {
    let writer = store.writer();
    let exec = move |actor: Actor, cmd: WriteCommand| writer.exec(actor, cmd);
    run_on(store, &exec, started_by, origin, plan, cancel, None).await
}

/// [`run_plan`] with a tool runner: tests only (story 3.3, owner condition
/// 2). Shipped code never names it (`cargo xtask seam-guard`); its one
/// runner is am-testkit's fixture tool. It builds the ToolGate over the
/// runner, and the driver holds only that gate (review R7). A runner that
/// declares one tool id twice is refused before any run is created.
pub async fn run_plan_with_tools(
    store: &Store,
    started_by: Actor,
    origin: &str,
    plan: &RunPlan,
    cancel: CancellationToken,
    tools: Arc<dyn ToolRunner>,
) -> Result<RunOutcome, RunError> {
    let refusals = Arc::new(Refusals::default());
    let gate = ToolGate::over(tools, refusals.clone());
    let twice = gate.duplicates();
    if !twice.is_empty() {
        return Err(RunError::Tools(format!(
            "its runner declares the tool id(s) {} more than once, so which manifest decides is not known; no run was created",
            twice.join(", ")
        )));
    }
    let writer = store.writer();
    let exec = move |actor: Actor, cmd: WriteCommand| writer.exec(actor, cmd);
    let tooling = Tooling::new(gate, refusals);
    run_on(
        store,
        &exec,
        started_by,
        origin,
        plan,
        cancel,
        Some(tooling),
    )
    .await
}

/// [`run_plan`], with every write command executed by `exec` (the seam for
/// the tests of a store that fails a command) and the tools (a ToolGate),
/// if any.
async fn run_on(
    store: &Store,
    exec: Exec<'_>,
    started_by: Actor,
    origin: &str,
    plan: &RunPlan,
    cancel: CancellationToken,
    tools: Option<Tooling>,
) -> Result<RunOutcome, RunError> {
    if plan.steps.is_empty() {
        return Err(RunError::NoSteps(plan.workflow.clone()));
    }
    let run_id = new_id();
    // A tool call acts as `Actor::Seat` of this run, named by its UUID: an
    // id that does not parse is an error, never a made-up UUID (review P13).
    let run_uuid = uuid::Uuid::parse_str(&run_id)
        .map_err(|e| StoreError::Invalid(format!("the new run id {run_id} is not a UUID: {e}")))?;
    exec(
        started_by,
        WriteCommand::CreateRun {
            run_id: run_id.clone(),
            workflow: plan.workflow.clone(),
            origin: origin.into(),
            steps: plan.steps.iter().map(|s| s.key.clone()).collect(),
            config_hash: Some(plan.config_hash.clone()),
        },
    )?;
    let run = Run {
        store,
        exec,
        run_id: &run_id,
        run_uuid,
        plan,
        cancel,
        tools: tools.as_ref(),
    };
    run.drive_to_outcome(Progress::default()).await
}

/// Story 3.4 (AD-8, AD-15, AD-16, AD-36; Question 1): goes on with the run
/// `run_id`, which stopped cleanly (at a gate a person has resolved, or
/// between steps), on `plan`, built from the snapshot the run pinned. Only
/// then: a decision a person committed on the waiting gate and the engine
/// has not applied yet is applied first, as `System(engine)`
/// ([`crate::gate::apply_resolution`]), and the run is driven from the
/// journal as [`run_plan`] drives it, to its end or the next gate.
///
/// Refused before anything is written: an unknown run, or one that has
/// finished ([`RunError::NotContinuable`]); a plan from another snapshot
/// ([`RunError::ConfigChanged`], naming both hashes); a step left
/// `running` (not a resolved gate), a model call unsettled or a tool call
/// in flight (resume is story 3.9: [`RunError::NotContinuable`]). An open
/// waiting gate is no decision: the run still waits, and nothing changes.
pub async fn continue_run(
    store: &Store,
    run_id: &str,
    plan: &RunPlan,
    cancel: CancellationToken,
) -> Result<RunOutcome, RunError> {
    if plan.steps.is_empty() {
        return Err(RunError::NoSteps(plan.workflow.clone()));
    }
    let reader = store.reader()?;
    let Some(run) = reader.run(run_id)? else {
        return Err(RunError::NotContinuable(format!(
            "there is no run {run_id} in this project, so there is nothing to continue"
        )));
    };
    if run.finished_at.is_some()
        || matches!(run.status.as_str(), "succeeded" | "failed" | "cancelled")
    {
        return Err(RunError::NotContinuable(format!(
            "run {run_id} has finished ({}), so there is nothing to continue",
            run.status
        )));
    }
    // R6: the plan is the run's own workflow's.
    if plan.workflow != run.workflow {
        return Err(RunError::NotContinuable(format!(
            "run {run_id} is a run of the workflow `{}`, and the plan given is for `{}`; nothing was changed",
            run.workflow, plan.workflow
        )));
    }
    if run.config_hash.as_deref() != Some(plan.config_hash.as_str()) {
        return Err(RunError::ConfigChanged {
            pinned: run.config_hash.unwrap_or_else(|| "no snapshot".into()),
            now: plan.config_hash.clone(),
        });
    }
    let decided = ready_to_go_on(&reader, run_id)?;
    let run_uuid = uuid::Uuid::parse_str(run_id)
        .map_err(|e| StoreError::Invalid(format!("the run id {run_id} is not a UUID: {e}")))?;
    let mut progress = Progress::default();
    if let Some(item) = decided {
        let applied = crate::gate::apply_resolution(&store.writer(), &reader, &item)?;
        progress.reason = applied.reason;
        progress.item = applied.item;
        if applied.run_failed {
            progress.finishing = Some("failed");
        }
    }
    drop(reader);
    let writer = store.writer();
    let exec = move |actor: Actor, cmd: WriteCommand| writer.exec(actor, cmd);
    let run = Run {
        store,
        exec: &exec,
        run_id,
        run_uuid,
        plan,
        cancel,
        tools: None,
    };
    run.drive_to_outcome(progress).await
}

/// Story 3.4 (review R2): whether the run `run_id` stopped cleanly enough
/// for [`continue_run`], read-only: no step `running` but at most one gate
/// whose Review a person resolved and the engine has not applied yet (its
/// item's id is given back, to apply first), no model call unsettled and
/// no tool call in flight. `agentic gate` and the API's resolve ask it
/// before they write an approve or a return, so a decision is never
/// recorded on a run that cannot go on. Refused in words
/// ([`RunError::NotContinuable`], naming story 3.9's resume).
pub fn ready_to_go_on(reader: &Reader, run_id: &str) -> Result<Option<String>, RunError> {
    let rows = reader.steps(run_id)?;
    let items = reader.inbox_items(run_id)?;
    let resume = |what: String| {
        RunError::NotContinuable(format!(
            "{what}, and going on from there (resume) arrives with story 3.9; nothing was changed"
        ))
    };
    let mut decided = None;
    for row in rows
        .iter()
        .filter(|r| r.status == StepStatus::Running.as_str())
    {
        let resolved = items.iter().find(|item| {
            item.step_run_id == row.id
                && item.kind == InboxKind::Review
                && item.status == "resolved"
        });
        match resolved {
            Some(item) if decided.is_none() => decided = Some(item.id.clone()),
            _ => {
                return Err(resume(format!(
                    "step `{}` attempt {} of run {run_id} was left running",
                    row.step_key, row.attempt
                )));
            }
        }
    }
    if let Some(call) = reader.calls_for_run(run_id)?.iter().find(|c| !c.settled) {
        return Err(resume(format!(
            "the model call {} of run {run_id} never settled",
            call.id
        )));
    }
    for row in &rows {
        if let Some(call) = reader
            .tool_calls(&row.id)?
            .into_iter()
            .find(|t| t.status == "issued")
        {
            return Err(resume(format!(
                "the tool call {} of step `{}` of run {run_id} was left in flight",
                call.id, row.step_key
            )));
        }
    }
    Ok(decided)
}

/// How a run's drive stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    Succeeded,
    Failed,
    /// A step waits for a person; the run row stays `running`.
    Waiting,
}

impl Ended {
    fn as_str(self) -> &'static str {
        match self {
            Ended::Succeeded => "succeeded",
            Ended::Failed => "failed",
            Ended::Waiting => StepStatus::WaitingHuman.as_str(),
        }
    }
}

/// How one step attempt ended for the drive loop.
enum StepEnd {
    /// It succeeded or looped back: the loop reads the journal again.
    Moved,
    /// It failed, and the run is finished `failed`.
    Failed,
    /// It waits for a person.
    Waiting,
}

/// The tools of a run made with a runner: the ToolGate
/// [`run_plan_with_tools`] built over it (the only holder and caller of the
/// runner, AD-11), the refusals ToolGate audited (written to the store
/// after each check) and the specs the model is offered.
struct Tooling {
    gate: ToolGate,
    refusals: Arc<Refusals>,
    specs: Vec<ToolSpec>,
}

impl Tooling {
    fn new(gate: ToolGate, refusals: Arc<Refusals>) -> Tooling {
        let specs = gate
            .manifests()
            .iter()
            .map(|m| ToolSpec {
                name: m.name.clone(),
                description: format!("The tool `{}`.", m.name),
                input_schema_json: "{\"type\":\"object\"}".into(),
            })
            .collect();
        Tooling {
            gate,
            refusals,
            specs,
        }
    }
}

/// ToolGate's audit sink: the refusals it records, written to the store by
/// the driver right after the check.
#[derive(Default)]
struct Refusals(Mutex<Vec<AuditEvent>>);

impl AuditSink for Refusals {
    fn record(&self, event: AuditEvent) {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).push(event);
    }
}

impl Refusals {
    fn take(&self) -> Vec<AuditEvent> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

/// One run being driven.
struct Run<'a> {
    store: &'a Store,
    exec: Exec<'a>,
    run_id: &'a str,
    run_uuid: uuid::Uuid,
    plan: &'a RunPlan,
    cancel: CancellationToken,
    tools: Option<&'a Tooling>,
}

/// The latest attempt of each step key among `rows`.
fn latest(rows: &[StepRow]) -> BTreeMap<&str, &StepRow> {
    let mut out: BTreeMap<&str, &StepRow> = BTreeMap::new();
    for row in rows {
        let keep = out
            .get(row.step_key.as_str())
            .is_none_or(|seen| seen.attempt < row.attempt);
        if keep {
            out.insert(row.step_key.as_str(), row);
        }
    }
    out
}

/// The reasons a loop-back step's final message lists: its JSON object's
/// `field`, a list of strings. Anything else is refused in words.
fn reasons_in(output: &str, field: &str) -> Result<Vec<String>, String> {
    let shape = format!(
        "it declares `loop_back`, so its final message must be a JSON object whose `{field}` field is a list of strings (an empty list moves on)"
    );
    let value: serde_json::Value = serde_json::from_str(output.trim())
        .map_err(|_| format!("{shape}; its final message is not JSON"))?;
    let Some(object) = value.as_object() else {
        return Err(format!("{shape}; its final message is not a JSON object"));
    };
    let Some(list) = object.get(field) else {
        return Err(format!("{shape}; the object has no `{field}` field"));
    };
    let Some(items) = list.as_array() else {
        return Err(format!("{shape}; its `{field}` field is not a list"));
    };
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("{shape}; an item of its `{field}` field is not text"))
        })
        .collect()
}

/// The reasons as one input's text, one per line.
fn reasons_text(reasons: &[String]) -> String {
    reasons
        .iter()
        .map(|r| format!("- {r}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A step attempt's inputs: the rows its start journals (the previous
/// step's output, then the plan's inputs; a loop-back target's reasons are
/// journaled already, by the loop-back) and the request's parts (trusted
/// first, then fenced; within each, the previous output, the reasons, the
/// plan's inputs), with the attempt's taint.
struct Inputs {
    rows: Vec<StepInput>,
    trusted: Vec<Part>,
    fenced: Vec<Part>,
    taint: Taint,
    /// The previous step's attempt, by `next` (a gate's payload).
    previous: Option<StepRow>,
    /// Why the attempt must not start (story 3.4: a gate note no person
    /// wrote), in words.
    refused: Option<String>,
}

impl Inputs {
    /// An input to journal with the attempt's start, and its part.
    fn add(&mut self, row: StepInput, part: Part) {
        self.place(row.taint, part);
        self.rows.push(row);
    }

    /// The part of an input the journal already holds for the attempt (a
    /// loop-back target's `loop_back` input), with its taint.
    fn place(&mut self, taint: Taint, part: Part) {
        self.taint = self.taint.raise(taint);
        match part {
            Part::Trusted(_) => self.trusted.push(part),
            other => self.fenced.push(other),
        }
    }
}

impl Run<'_> {
    fn exec(&self, actor: Actor, cmd: WriteCommand) -> Result<(), StoreError> {
        (self.exec)(actor, cmd)
    }

    /// Drives the run from `progress` and says how it ended, or where it
    /// waits. A store error once the run exists never leaves it `running`
    /// unsaid (see [`RunError::Stopped`]).
    async fn drive_to_outcome(&self, mut progress: Progress) -> Result<RunOutcome, RunError> {
        match self.drive(&mut progress).await {
            Ok(ended) => {
                // A run that already waited (continued with no decision)
                // names the item it waits on.
                if ended == Ended::Waiting && progress.item.is_none() {
                    progress.item = self.waiting_item()?;
                }
                Ok(RunOutcome {
                    run_id: self.run_id.to_string(),
                    status: ended.as_str().into(),
                    reason: progress.reason,
                    item: progress.item,
                })
            }
            Err(source) => {
                let to_record = progress.to_record();
                if let Some(step_run_id) = progress.open_step {
                    let _ = self.exec(
                        engine(),
                        WriteCommand::FinishStep {
                            step_run_id,
                            status: "failed".into(),
                            output: None,
                        },
                    );
                }
                let recorded = self.exec(
                    engine(),
                    WriteCommand::FinishRun {
                        run_id: self.run_id.to_string(),
                        status: to_record.into(),
                    },
                );
                Err(RunError::Stopped {
                    run_id: self.run_id.to_string(),
                    status: if recorded.is_ok() {
                        to_record
                    } else {
                        "running"
                    }
                    .into(),
                    reason: progress.reason,
                    source: Box::new(source),
                })
            }
        }
    }

    /// The open item a waiting step of the run waits on.
    fn waiting_item(&self) -> Result<Option<String>, StoreError> {
        let reader = self.store.reader()?;
        let waiting: Vec<String> = reader
            .steps(self.run_id)?
            .into_iter()
            .filter(|s| s.status == StepStatus::WaitingHuman.as_str())
            .map(|s| s.id)
            .collect();
        Ok(reader
            .inbox_items(self.run_id)?
            .into_iter()
            .find(|i| i.status == "open" && waiting.contains(&i.step_run_id))
            .map(|i| i.id))
    }

    fn steps(&self) -> Result<Vec<StepRow>, StoreError> {
        self.store.reader()?.steps(self.run_id)
    }

    /// Drives the created run: each time, from the journal, the first step
    /// whose latest attempt is `queued`, until every step succeeded, one
    /// failed or one waits.
    async fn drive(&self, progress: &mut Progress) -> Result<Ended, StoreError> {
        loop {
            let rows = self.steps()?;
            let latest = latest(&rows);
            let mut next = None;
            for (i, step) in self.plan.steps.iter().enumerate() {
                let Some(row) = latest.get(step.key.as_str()) else {
                    return Err(StoreError::Invalid(format!(
                        "the journal of run {} holds no step `{}`, which its plan has",
                        self.run_id, step.key
                    )));
                };
                match StepStatus::parse(&row.status) {
                    Some(StepStatus::Succeeded) => continue,
                    Some(StepStatus::Queued) => {
                        next = Some((i, (*row).clone()));
                        break;
                    }
                    Some(StepStatus::WaitingHuman) => return Ok(Ended::Waiting),
                    Some(StepStatus::Failed | StepStatus::Cancelled) => return Ok(Ended::Failed),
                    _ => {
                        return Err(StoreError::Invalid(format!(
                            "step `{}` attempt {} of run {} is {}, and this version does not resume a run from there",
                            row.step_key, row.attempt, self.run_id, row.status
                        )));
                    }
                }
            }
            let Some((i, row)) = next else {
                progress.finishing = Some("succeeded");
                self.finish_run("succeeded")?;
                return Ok(Ended::Succeeded);
            };
            match self.run_step(i, &row, &rows, progress).await? {
                StepEnd::Moved => continue,
                StepEnd::Failed => return Ok(Ended::Failed),
                StepEnd::Waiting => return Ok(Ended::Waiting),
            }
        }
    }

    fn finish_run(&self, status: &str) -> Result<(), StoreError> {
        self.exec(
            engine(),
            WriteCommand::FinishRun {
                run_id: self.run_id.into(),
                status: status.into(),
            },
        )
    }

    /// Step `i`'s inputs for its attempt `row`, from the journal `rows`:
    /// every producer's text and taint as committed (AD-10: never
    /// recomputed).
    fn inputs_of(&self, i: usize, row: &StepRow, rows: &[StepRow]) -> Result<Inputs, StoreError> {
        let mut inputs = Inputs {
            rows: Vec::new(),
            trusted: Vec::new(),
            fenced: Vec::new(),
            taint: Taint::Clean,
            previous: None,
            refused: None,
        };
        let latest = latest(rows);
        // `next`: the previous step's latest attempt, which succeeded; across
        // an approved gate, the output that gate's Review carried (story
        // 3.4, Decision 4: the producer's attempt, with its committed taint).
        if let Some(prev) = i
            .checked_sub(1)
            .and_then(|p| self.previous_output(p, &latest, rows).transpose())
            .transpose()?
        {
            inputs.previous = Some(prev.clone());
            let prev = &prev;
            if let Some(output) = &prev.output {
                let label = format!("step:{}@{}", prev.step_key, prev.attempt);
                let part =
                    TrustedInput::from_step_output(label.clone(), output.clone(), prev.taint);
                inputs.add(
                    StepInput {
                        kind: "step_output".into(),
                        from_step_run: Some(prev.id.clone()),
                        from_item: None,
                        label,
                        taint: prev.taint,
                    },
                    part,
                );
            }
        }
        // A loop-back target's new attempt: the producer's reasons, from the
        // `loop_back` input the loop-back journaled for it (AD-43: the edge
        // is stored once).
        let journaled = self.store.reader()?.step_inputs(&row.id)?;
        for producer in journaled
            .iter()
            .filter(|input| input.kind == "loop_back")
            .filter_map(|input| input.from_step_run.as_ref())
            .filter_map(|id| rows.iter().find(|r| &r.id == id))
        {
            let field = self
                .plan
                .steps
                .iter()
                .find_map(|s| match &s.hand_off {
                    HandOff::LoopBack { field, .. } if s.key == producer.step_key => {
                        Some(field.as_str())
                    }
                    _ => None,
                })
                .unwrap_or("reasons");
            let output = producer.output.clone().unwrap_or_default();
            let text = reasons_in(&output, field)
                .map(|reasons| reasons_text(&reasons))
                .unwrap_or(output);
            let label = format!("loop-back:{}@{}", producer.step_key, producer.attempt);
            let part = TrustedInput::from_step_output(label, text, producer.taint);
            inputs.place(producer.taint, part);
        }
        // Story 3.4 (AD-10 (b), AD-27, AD-43): a returned producer's new
        // attempt receives the person's note, from the `gate_note` input
        // the apply journaled for it: trusted only through
        // `from_owner_note` with the resolver the journal holds, after the
        // other trusted inputs and before the fence. It neither adds nor
        // clears taint.
        for input in journaled.iter().filter(|input| input.kind == "gate_note") {
            match self.gate_note(input)? {
                Ok(note) => inputs.place(Taint::Clean, Part::Trusted(note)),
                Err(why) => {
                    inputs.refused = Some(why);
                    return Ok(inputs);
                }
            }
        }
        // The plan's own inputs: untrusted, so every attempt is tainted.
        for data in &self.plan.steps[i].inputs {
            inputs.add(
                StepInput {
                    kind: "untrusted".into(),
                    from_step_run: None,
                    from_item: None,
                    label: data.label().to_string(),
                    taint: Taint::Tainted,
                },
                Part::Untrusted(data.clone()),
            );
        }
        Ok(inputs)
    }

    /// The output step `p` hands the next step by `next`: its latest
    /// attempt when that succeeded; for an approved gate, the attempt its
    /// Review item carried (story 3.4, Decision 4). `None` when there is
    /// none.
    fn previous_output(
        &self,
        p: usize,
        latest: &BTreeMap<&str, &StepRow>,
        rows: &[StepRow],
    ) -> Result<Option<StepRow>, StoreError> {
        let step = &self.plan.steps[p];
        let Some(prev) = latest
            .get(step.key.as_str())
            .filter(|row| row.status == StepStatus::Succeeded.as_str())
        else {
            return Ok(None);
        };
        if !matches!(step.hand_off, HandOff::Gate { .. }) {
            return Ok(Some((*prev).clone()));
        }
        let payload = self
            .store
            .reader()?
            .inbox_items(self.run_id)?
            .into_iter()
            .find(|item| item.step_run_id == prev.id && item.kind == InboxKind::Review)
            .and_then(|item| item.payload);
        Ok(payload.and_then(|id| rows.iter().find(|r| r.id == id).cloned()))
    }

    /// The trusted part of a `gate_note` input (story 3.4, AD-10 (b)): the
    /// note on the item it names, trusted only through
    /// `TrustedInput::from_owner_note` with the resolver as committed in
    /// `resolved_by`. `Err` (in words) when the item holds no note or no
    /// person resolved it: the attempt must not start.
    fn gate_note(&self, input: &StepInput) -> Result<Result<TrustedInput, String>, StoreError> {
        let Some(item_id) = &input.from_item else {
            return Ok(Err(format!(
                "its gate note `{}` names no Inbox item",
                input.label
            )));
        };
        let Some(item) = self.store.reader()?.inbox_item(item_id)? else {
            return Ok(Err(format!(
                "its gate note names the Inbox item {item_id}, which does not exist"
            )));
        };
        // The resolver as committed (AD-10 (b)): never recomputed.
        let resolver = item.resolver();
        let (Some(resolver), Some(note)) = (resolver, item.note) else {
            return Ok(Err(format!(
                "its gate note's Inbox item {item_id} holds no note from a person"
            )));
        };
        match TrustedInput::from_owner_note(input.label.clone(), note, &resolver) {
            Some(trusted) => Ok(Ok(trusted)),
            None => Ok(Err(format!(
                "its gate note's Inbox item {item_id} was resolved by {}, not by a person, so the note is not trusted and the step does not start (AD-10)",
                resolver.describe()
            ))),
        }
    }

    /// Runs step `i`'s queued attempt `row` to its end, or to its gate.
    async fn run_step(
        &self,
        i: usize,
        row: &StepRow,
        rows: &[StepRow],
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        let step = &self.plan.steps[i];
        let inputs = self.inputs_of(i, row, rows)?;
        if let Some(why) = &inputs.refused {
            // Fail closed (AD-10 (b)): an attempt whose gate note no person
            // wrote never starts; the run stops with one Failure item in
            // the engine's words, never the note (review R5).
            progress.reason = Some(format!("step `{}` did not start: {why}", step.key));
            let item = NewInboxItem {
                id: new_id(),
                kind: InboxKind::Failure,
                payload: None,
                taint: Taint::Clean,
                title: format!("Step {} did not start", step.key),
                body: format!(
                    "Step `{}` did not start, so the run failed: {why}. Step run {} stays queued.",
                    step.key, row.id
                ),
                max_returns: None,
            };
            let item_id = item.id.clone();
            progress.finishing = Some("failed");
            self.exec(
                engine(),
                WriteCommand::FailBeforeStart {
                    step_run_id: row.id.clone(),
                    item,
                },
            )?;
            progress.item = Some(item_id);
            return Ok(StepEnd::Failed);
        }
        if let HandOff::Gate { max_returns } = step.hand_off {
            return self.pause_at_gate(step, row, inputs, max_returns, progress);
        }
        let Inputs {
            rows: journaled,
            trusted,
            fenced,
            taint,
            ..
        } = inputs;
        // Committed before the first call (AD-8): the attempt's start, its
        // inputs and its taint.
        self.exec(
            engine(),
            WriteCommand::StartAttempt {
                step_run_id: row.id.clone(),
                inputs: journaled,
                taint,
            },
        )?;
        progress.open_step = Some(row.id.clone());
        // A fresh attempt's whole cap, from tokio's clock right after its
        // start committed (review P1; story 3.9's resume will count a
        // resumed attempt's persisted `started_at`).
        let deadline = tokio::time::Instant::now() + step.limits.wall_clock;
        let mut parts = vec![Part::Instruction(step.instruction.clone())];
        parts.extend(trusted);
        parts.extend(fenced);
        let mut messages = vec![Message {
            role: Role::User,
            parts,
        }];
        let seat = Actor::Seat {
            seat: step.seat.clone(),
            run: self.run_uuid,
            step: step.key.clone(),
        };
        // The taint the next reservation carries: the attempt's, then
        // tainted once a tool result is appended.
        let mut carry = taint;
        loop {
            // Nothing starts once the deadline has passed (review P7).
            if tokio::time::Instant::now() >= deadline {
                return self.fail_with_item(
                    step,
                    row,
                    carry,
                    None,
                    None,
                    Cap::WallClock(step.limits.wall_clock),
                    progress,
                );
            }
            let made = self.store.reader()?.calls_for_step(&row.id)?.len() as i64;
            let call_id = new_id();
            self.exec(
                engine(),
                WriteCommand::ReserveCall {
                    step_run_id: row.id.clone(),
                    call_id: call_id.clone(),
                    call_seq: made + 1,
                    reserved_micro_usd: 0,
                    rate_snapshot: NO_RATE_SNAPSHOT.into(),
                    taint: carry,
                },
            )?;
            let request = ChatRequest {
                model: step.model.clone(),
                system: step.system.clone(),
                messages: messages.clone(),
                tools: self.tools.map(|t| t.specs.clone()).unwrap_or_default(),
                max_output_tokens: step.max_output_tokens,
            };
            let (turn, timed_out) = self.call_within(step, request, deadline).await;
            if timed_out {
                let usage = turn.as_ref().ok().and_then(|out| out.usage);
                return self.fail_with_item(
                    step,
                    row,
                    carry,
                    Some(CallSettlement {
                        call_id,
                        usage,
                        cost_micro_usd: None,
                    }),
                    None,
                    Cap::WallClock(step.limits.wall_clock),
                    progress,
                );
            }
            if let Some(why) = failure_of(step, &turn) {
                let usage = turn.as_ref().ok().and_then(|out| out.usage);
                return self.fail(step, row, Some((call_id, usage)), why, progress);
            }
            let Ok(out) = turn else {
                unreachable!("failure_of names every provider error");
            };
            if out.tool_calls.is_empty() {
                return self.end_turn(step, row, call_id, out, carry, progress);
            }
            let Some(tools) = self.tools else {
                let why = tool_words(&out);
                return self.fail(step, row, Some((call_id, out.usage)), why, progress);
            };
            // The iteration cap, from the journal: a turn that asks for
            // tools needs one more turn, which the cap may not allow.
            let made = self.store.reader()?.calls_for_step(&row.id)?.len() as u32;
            if made >= step.limits.iterations {
                return self.fail_with_item(
                    step,
                    row,
                    carry,
                    Some(CallSettlement {
                        call_id,
                        usage: out.usage,
                        cost_micro_usd: None,
                    }),
                    None,
                    Cap::Iterations(step.limits.iterations),
                    progress,
                );
            }
            self.exec(
                engine(),
                WriteCommand::SettleCall {
                    call_id,
                    usage: out.usage,
                    cost_micro_usd: None,
                },
            )?;
            let mut results = Vec::new();
            for call in &out.tool_calls {
                match self
                    .tool_call(tools, step, row, &seat, call, carry, deadline, progress)
                    .await?
                {
                    ToolEnd::Result(content) => results.push(Part::ToolResult {
                        call_id: call.id.clone(),
                        content,
                    }),
                    ToolEnd::Stopped(end) => return Ok(end),
                }
            }
            let mut said = Vec::new();
            if !out.text.is_empty() {
                said.push(Part::AssistantText(out.text.clone()));
            }
            said.extend(out.tool_calls.iter().cloned().map(Part::ToolCall));
            messages.push(Message {
                role: Role::Assistant,
                parts: said,
            });
            messages.push(Message {
                role: Role::User,
                parts: results,
            });
            // Tool results are untrusted (AD-10): the attempt is tainted,
            // committed with the next call's reservation.
            carry = Taint::Tainted;
        }
    }

    /// One model call, cancelled through its own child token when the
    /// wall-clock `deadline` passes (the run's token is untouched); `true`
    /// when that happened. A reply ready at the deadline loses to it.
    async fn call_within(
        &self,
        step: &StepPlan,
        request: ChatRequest,
        deadline: tokio::time::Instant,
    ) -> (Result<ChatOutcome, ProviderError>, bool) {
        let child = self.cancel.child_token();
        let mut call = step.provider.stream(
            request,
            TokenSignal::signal(child.clone()),
            Arc::new(NullSink),
        );
        tokio::select! {
            biased;
            () = tokio::time::sleep_until(deadline) => {
                child.cancel();
                match tokio::time::timeout(CANCEL_GRACE, call).await {
                    Ok(turn) => (turn, true),
                    // A call that does not stop settles at its reservation.
                    Err(_) => (
                        Ok(ChatOutcome {
                            text: String::new(),
                            tool_calls: vec![],
                            usage: None,
                            stop: StopReason::Cancelled,
                        }),
                        true,
                    ),
                }
            }
            turn = &mut call => (turn, false),
        }
    }

    /// One tool call the model asked for: the deadline, the tool-call cap
    /// from the journal, then ToolGate's admission (uses counted per step
    /// run), then the call journaled `issued` before ToolGate runs it,
    /// within the wall-clock `deadline`. The name is journaled and audited
    /// as [`tool_label`] gives it.
    #[allow(clippy::too_many_arguments)]
    async fn tool_call(
        &self,
        tools: &Tooling,
        step: &StepPlan,
        row: &StepRow,
        seat: &Actor,
        call: &ToolCall,
        carry: Taint,
        deadline: tokio::time::Instant,
        progress: &mut Progress,
    ) -> Result<ToolEnd, StoreError> {
        // Nothing starts once the deadline has passed (review P7).
        if tokio::time::Instant::now() >= deadline {
            let end = self.fail_with_item(
                step,
                row,
                carry,
                None,
                None,
                Cap::WallClock(step.limits.wall_clock),
                progress,
            )?;
            return Ok(ToolEnd::Stopped(end));
        }
        // The model's name for the tool, as the journal may hold it.
        let call = &ToolCall {
            name: tools.gate.label(&call.name),
            ..call.clone()
        };
        let journaled = self.store.reader()?.tool_calls(&row.id)?;
        let used = journaled.iter().filter(|t| t.status != "refused").count() as u32;
        let record = |status: &str| WriteCommand::RecordToolCall {
            tool_call_id: new_id(),
            step_run_id: row.id.clone(),
            call_seq: journaled.len() as i64 + 1,
            tool: call.name.clone(),
            status: status.into(),
        };
        if used >= step.limits.tool_calls {
            self.exec(engine(), record("refused"))?;
            let end = self.fail_with_item(
                step,
                row,
                carry,
                None,
                None,
                Cap::ToolCalls(step.limits.tool_calls),
                progress,
            )?;
            return Ok(ToolEnd::Stopped(end));
        }
        let admitted = tools.gate.admit(seat, &row.id, call);
        for event in tools.refusals.take() {
            self.exec(
                event.actor,
                WriteCommand::Audit {
                    action: event.action,
                    target: event.target,
                    outcome: event.outcome,
                    hash: event.hash,
                    args: event.args,
                },
            )?;
        }
        let permit = match admitted {
            Ok(permit) => permit,
            Err(why) => {
                self.exec(engine(), record("refused"))?;
                let end = self.fail(step, row, None, why, progress)?;
                return Ok(ToolEnd::Stopped(end));
            }
        };
        let issued = record("issued");
        let WriteCommand::RecordToolCall { tool_call_id, .. } = &issued else {
            unreachable!("record builds a RecordToolCall");
        };
        let tool_call_id = tool_call_id.clone();
        // Committed before the tool runs (AD-8).
        self.exec(engine(), issued)?;
        let child = self.cancel.child_token();
        let running = tools.gate.run(permit, TokenSignal::signal(child.clone()));
        // A tool done at the deadline loses to it (review P7).
        let result = tokio::select! {
            biased;
            () = tokio::time::sleep_until(deadline) => {
                child.cancel();
                None
            }
            result = running => Some(result),
        };
        let finish = |status: &str| WriteCommand::FinishToolCall {
            tool_call_id: tool_call_id.clone(),
            status: status.into(),
        };
        match result {
            None => {
                self.exec(engine(), finish("failed"))?;
                let end = self.fail_with_item(
                    step,
                    row,
                    carry,
                    None,
                    None,
                    Cap::WallClock(step.limits.wall_clock),
                    progress,
                )?;
                Ok(ToolEnd::Stopped(end))
            }
            Some(Ok(content)) => {
                self.exec(engine(), finish("succeeded"))?;
                Ok(ToolEnd::Result(content))
            }
            Some(Err(why)) => {
                self.exec(engine(), finish("failed"))?;
                // The tool's own words are untrusted too (AD-10).
                Ok(ToolEnd::Result(UntrustedData::text(
                    format!("tool:{}", call.name),
                    why,
                )))
            }
        }
    }

    /// A turn that ended without asking for tools: its step's hand-off.
    fn end_turn(
        &self,
        step: &StepPlan,
        row: &StepRow,
        call_id: String,
        out: ChatOutcome,
        carry: Taint,
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        let HandOff::LoopBack { to, field, max } = &step.hand_off else {
            return self.succeed(row, call_id, out, progress);
        };
        let reasons = match reasons_in(&out.text, field) {
            Ok(reasons) => reasons,
            Err(why) => return self.fail(step, row, Some((call_id, out.usage)), why, progress),
        };
        if reasons.is_empty() {
            return self.succeed(row, call_id, out, progress);
        }
        // The count is the journal's: this step's attempts that looped back.
        let looped = self
            .steps()?
            .iter()
            .filter(|s| s.step_key == step.key && s.status == StepStatus::LoopedBack.as_str())
            .count() as u32;
        if looped >= *max {
            return self.fail_with_item(
                step,
                row,
                carry,
                Some(CallSettlement {
                    call_id,
                    usage: out.usage,
                    cost_micro_usd: None,
                }),
                Some(out.text),
                Cap::LoopBack {
                    to: to.clone(),
                    max: *max,
                },
                progress,
            );
        }
        // The settlement, the attempt's end and the new attempts commit
        // together (AD-8).
        self.exec(
            engine(),
            WriteCommand::SettleAndLoopBack {
                call_id,
                usage: out.usage,
                cost_micro_usd: None,
                step_run_id: row.id.clone(),
                output: out.text,
                to: to.clone(),
            },
        )?;
        progress.open_step = None;
        Ok(StepEnd::Moved)
    }

    fn succeed(
        &self,
        row: &StepRow,
        call_id: String,
        out: ChatOutcome,
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        // The settlement and the step's finish commit together (AD-8).
        self.exec(
            engine(),
            WriteCommand::SettleAndFinishStep {
                call_id,
                usage: out.usage,
                cost_micro_usd: None,
                step_run_id: row.id.clone(),
                status: "succeeded".into(),
                output: Some(out.text),
            },
        )?;
        progress.open_step = None;
        Ok(StepEnd::Moved)
    }

    /// The step fails for `why` with no Inbox item (story 3.1's failures):
    /// the open call, if any, settles with the step's finish, then the run
    /// finishes `failed`. A failed turn's text is not stored.
    fn fail(
        &self,
        step: &StepPlan,
        row: &StepRow,
        call: Option<(String, Option<am_domain::Usage>)>,
        why: String,
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        progress.reason = Some(format!("step `{}` failed: {why}", step.key));
        let finish = match call {
            Some((call_id, usage)) => WriteCommand::SettleAndFinishStep {
                call_id,
                usage,
                cost_micro_usd: None,
                step_run_id: row.id.clone(),
                status: "failed".into(),
                output: None,
            },
            None => WriteCommand::FinishStep {
                step_run_id: row.id.clone(),
                status: "failed".into(),
                output: None,
            },
        };
        self.exec(engine(), finish)?;
        progress.open_step = None;
        progress.finishing = Some("failed");
        self.finish_run("failed")?;
        Ok(StepEnd::Failed)
    }

    /// The step fails at a maximum (a cap, or the loop-back maximum): the
    /// call settles (if given), the step fails, its Failure item is
    /// created and the run finishes `failed`, in one transaction. The
    /// item's words are the engine's, never model text.
    #[allow(clippy::too_many_arguments)]
    fn fail_with_item(
        &self,
        step: &StepPlan,
        row: &StepRow,
        carry: Taint,
        settle: Option<CallSettlement>,
        output: Option<String>,
        cap: Cap,
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        progress.reason = Some(format!("step `{}` failed: {}", step.key, cap.reason()));
        let item = NewInboxItem {
            id: new_id(),
            kind: InboxKind::Failure,
            payload: Some(row.id.clone()),
            taint: carry,
            title: cap.title(&step.key),
            body: cap.body(&step.key, &row.id),
            max_returns: None,
        };
        let item_id = item.id.clone();
        progress.finishing = Some("failed");
        self.exec(
            engine(),
            WriteCommand::FailWithItem {
                step_run_id: row.id.clone(),
                settle,
                output,
                item,
            },
        )?;
        progress.open_step = None;
        progress.item = Some(item_id);
        Ok(StepEnd::Failed)
    }

    /// A gate step (AD-36): no model call. It moves from `queued` to
    /// `waiting_human` with its inputs and one Review item whose payload is
    /// the previous step's attempt, in one transaction. No model output
    /// decides it; only a Human resolve (story 3.4) moves it on.
    fn pause_at_gate(
        &self,
        step: &StepPlan,
        row: &StepRow,
        inputs: Inputs,
        max_returns: u32,
        progress: &mut Progress,
    ) -> Result<StepEnd, StoreError> {
        let (payload, payload_taint, body) = match &inputs.previous {
            Some(prev) => (
                Some(prev.id.clone()),
                prev.taint,
                format!(
                    "Step `{}` waits for your review of the output of step `{}` (attempt {}, step run {}). The run goes on only when you resolve this item.",
                    step.key, prev.step_key, prev.attempt, prev.id
                ),
            ),
            None => (
                None,
                Taint::Clean,
                format!(
                    "Step `{}` waits for your review. The run goes on only when you resolve this item.",
                    step.key
                ),
            ),
        };
        let item = NewInboxItem {
            id: new_id(),
            kind: InboxKind::Review,
            payload,
            taint: payload_taint.raise(inputs.taint),
            title: format!("Review before step {} goes on", step.key),
            body,
            // Journaled at the pause, so a decision is applied from the
            // journal alone (story 3.4, Decision 3).
            max_returns: Some(max_returns),
        };
        let item_id = item.id.clone();
        self.exec(
            engine(),
            WriteCommand::PauseStep {
                step_run_id: row.id.clone(),
                inputs: inputs.rows,
                taint: inputs.taint,
                item,
            },
        )?;
        progress.item = Some(item_id);
        Ok(StepEnd::Waiting)
    }
}

/// How a tool call ended for the loop.
enum ToolEnd {
    /// Its result, to append for the next turn.
    Result(UntrustedData),
    /// The step ended (a cap, a refusal).
    Stopped(StepEnd),
}

/// A maximum a step attempt reached.
enum Cap {
    Iterations(u32),
    ToolCalls(u32),
    WallClock(Duration),
    LoopBack { to: String, max: u32 },
}

impl Cap {
    /// Why the step failed, in words that name the maximum and its value.
    fn reason(&self) -> String {
        match self {
            Cap::Iterations(n) => format!(
                "it reached its iterations cap of {n} model turns (limits.iterations) and its last turn asked for more"
            ),
            Cap::ToolCalls(n) => format!(
                "it reached its tool_calls cap of {n} tool calls (limits.tool_calls) and asked for one more"
            ),
            Cap::WallClock(d) => format!(
                "it reached its wall_clock cap of {} (limits.wall_clock); the call in flight was cancelled",
                duration_words(*d)
            ),
            Cap::LoopBack { to, max } => format!(
                "it asked to loop back to step `{to}` again, past its maximum of {max} loop-backs (loop_back.max)"
            ),
        }
    }

    fn title(&self, key: &str) -> String {
        match self {
            Cap::LoopBack { .. } => format!("Step {key} looped back too often"),
            _ => format!("Step {key} stopped at a cap"),
        }
    }

    /// The item's body: the step, the maximum and its value, and the step
    /// run it points at; never the model's words.
    fn body(&self, key: &str, step_run: &str) -> String {
        match self {
            Cap::Iterations(n) => format!(
                "Step `{key}` stopped at its iterations cap of {n} model turns (limits.iterations) and failed, so the run failed. Step run {step_run} holds its calls."
            ),
            Cap::ToolCalls(n) => format!(
                "Step `{key}` stopped at its tool_calls cap of {n} tool calls (limits.tool_calls) and failed, so the run failed. Step run {step_run} holds its calls."
            ),
            Cap::WallClock(d) => format!(
                "Step `{key}` stopped at its wall_clock cap of {} (limits.wall_clock) and failed, so the run failed. Step run {step_run} holds its calls.",
                duration_words(*d)
            ),
            Cap::LoopBack { to, max } => format!(
                "Step `{key}` asked to loop back to step `{to}` past its maximum of {max} loop-backs (loop_back.max) and failed, so the run failed. Its reasons are in the output of its last attempt, step run {step_run}."
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plan with no steps is refused before anything is written.
    #[tokio::test]
    async fn a_plan_with_no_steps_creates_no_run() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let plan = RunPlan {
            workflow: "wf".into(),
            config_hash: "0".repeat(64),
            steps: vec![],
        };
        let refused = run_plan(
            &store,
            Actor::System {
                component: "test".into(),
            },
            "manual",
            &plan,
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(refused, Err(RunError::NoSteps(wf)) if wf == "wf"));
        assert!(store.reader().unwrap().runs().unwrap().is_empty());
    }

    // -------------------------------------------------------------- store errors
    //
    // The seam (second review of story 3.1): `run_on` takes the function
    // that executes a write command, so these tests run the driver on a
    // real store whose writer refuses the commands a test picks.

    use std::sync::atomic::{AtomicUsize, Ordering};

    use am_store::Writer;
    use am_testkit::{FakeLlm, ScriptedTurn};

    fn owner() -> Actor {
        Actor::Human {
            person: "owner".into(),
            hat: "Owner".into(),
        }
    }

    fn one_step(provider: Arc<dyn LlmProvider>) -> RunPlan {
        RunPlan {
            workflow: "wf".into(),
            config_hash: "0".repeat(64),
            steps: vec![StepPlan {
                key: "only".into(),
                seat: "seat-only".into(),
                system: vec![],
                instruction: Instruction::authored("Do the work."),
                inputs: vec![],
                hand_off: HandOff::Next,
                limits: Limits::default(),
                model: "example-local".into(),
                max_output_tokens: 64,
                provider,
            }],
        }
    }

    fn refused() -> StoreError {
        StoreError::Invalid("the disk is full".into())
    }

    /// Runs `plan` on `store` with a writer that refuses every command for
    /// which `refuse` says so; `refuse` is given the command and how many
    /// commands of the same audit action came before it.
    async fn run_refusing(
        store: &Store,
        plan: &RunPlan,
        refuse: impl Fn(&WriteCommand, usize) -> bool + Send + Sync,
    ) -> Result<RunOutcome, RunError> {
        let writer: Writer = store.writer();
        let (finishes, others) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let exec = move |actor: Actor, cmd: WriteCommand| {
            let seen = match &cmd {
                WriteCommand::FinishRun { .. } => finishes.fetch_add(1, Ordering::SeqCst),
                _ => others.fetch_add(1, Ordering::SeqCst),
            };
            if refuse(&cmd, seen) {
                return Err(refused());
            }
            writer.exec(actor, cmd)
        };
        run_on(
            store,
            &exec,
            owner(),
            "manual",
            plan,
            CancellationToken::new(),
            None,
        )
        .await
    }

    /// The last `FinishRun` fails once: every step succeeded, so the
    /// cleanup records what the run earned, `succeeded`, not `failed`, and
    /// the error says that is what the journal holds.
    #[tokio::test]
    async fn a_refused_last_finish_is_retried_with_the_status_the_run_earned() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let plan = one_step(Arc::new(FakeLlm::new(vec![ScriptedTurn::text("done")])));

        let stopped = run_refusing(&store, &plan, |cmd, seen| {
            matches!(cmd, WriteCommand::FinishRun { .. }) && seen == 0
        })
        .await
        .expect_err("the store refused a command");

        let RunError::Stopped {
            run_id,
            status,
            reason,
            source,
        } = &stopped
        else {
            panic!("{stopped:?}");
        };
        assert_eq!(status, "succeeded");
        assert_eq!(reason, &None);
        assert!(source.to_string().contains("the disk is full"), "{source}");
        let reader = store.reader().unwrap();
        let run = reader.run(run_id).unwrap().unwrap();
        assert_eq!(run.status, "succeeded");
        assert!(run.finished_at.is_some());
        let steps = reader.steps(run_id).unwrap();
        assert_eq!(steps[0].status, "succeeded");
        assert_eq!(steps[0].output.as_deref(), Some("done"));
    }

    /// The store refuses every `FinishRun`: the run stays `running`, and
    /// the error says so instead of naming a status that was not recorded.
    #[tokio::test]
    async fn a_store_that_refuses_every_finish_leaves_the_run_running_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("state.db")).unwrap();
        let plan = one_step(Arc::new(FakeLlm::new(vec![ScriptedTurn::text("done")])));

        let stopped = run_refusing(&store, &plan, |cmd, _| {
            matches!(cmd, WriteCommand::FinishRun { .. })
        })
        .await
        .expect_err("the store refused a command");

        let RunError::Stopped { run_id, status, .. } = &stopped else {
            panic!("{stopped:?}");
        };
        assert_eq!(status, "running");
        let run = store.reader().unwrap().run(run_id).unwrap().unwrap();
        assert_eq!(run.status, "running");
        assert_eq!(run.finished_at, None);
    }

    /// A step failed for the provider's reason, then the store refused a
    /// command (the settlement, or the run's finish): the error keeps the
    /// step's reason beside the store's, and the run is recorded `failed`.
    #[tokio::test]
    async fn a_store_error_after_a_failed_step_keeps_the_steps_reason() {
        type Refuse = fn(&WriteCommand, usize) -> bool;
        let cases: [(&str, Refuse); 2] = [
            ("the settlement", |cmd, _| {
                matches!(cmd, WriteCommand::SettleAndFinishStep { .. })
            }),
            ("the run's first finish", |cmd, seen| {
                matches!(cmd, WriteCommand::FinishRun { .. }) && seen == 0
            }),
        ];
        for (case, refuse) in cases {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open(&dir.path().join("state.db")).unwrap();
            let failing = FakeLlm::new(vec![ScriptedTurn::fail(am_domain::ProviderError::Http {
                status: 500,
                body: "upstream is down".into(),
            })]);
            let plan = one_step(Arc::new(failing));

            let stopped = run_refusing(&store, &plan, refuse)
                .await
                .expect_err("the store refused a command");

            let RunError::Stopped {
                run_id,
                status,
                reason,
                source,
            } = &stopped
            else {
                panic!("{case}: {stopped:?}");
            };
            assert_eq!(status, "failed", "{case}");
            let reason = reason.as_deref().expect("the step's reason is kept");
            assert!(
                reason.contains("only") && reason.contains("500"),
                "{case}: {reason}"
            );
            assert!(source.to_string().contains("the disk is full"), "{case}");
            let reader = store.reader().unwrap();
            assert_eq!(
                reader.run(run_id).unwrap().unwrap().status,
                "failed",
                "{case}"
            );
            assert_eq!(reader.steps(run_id).unwrap()[0].status, "failed", "{case}");
        }
    }
}
